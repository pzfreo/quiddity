# Recognition 6: file-coordinate measurements (for #92)

Readers in draftwright and Specify should accept `quiddity-rust/recognition/6`.
Every published record is usable without the top-level `frame`, which remains recognition
provenance. There is no `local` key, including on nested records. Schema 6 publishes one
family per feature kind: oriented slots, chamfers and slot patterns join their plain
families. Recognition predicates, proofs, frame voting and accepted counts are unchanged.
The crate version and fingerprint format `quiddity-rust/fingerprint/3` are unchanged;
merged records' fingerprint values change within v3, as described below.

An axis-valued field can be a file-axis letter (`"x"`, `"y"`, `"z"`) or a unit vector
`[x,y,z]` in file coordinates. This applies to `axis`, `long_axis`, `width_axis`,
`depth_axis`, `other_axis`, `thickness_axis`, nested `profile.axis`, support-plane axes
and mirror-plane axes. Existing vector-valued directions retain their contracts.
Only records that previously had frame-relative fields acquire new representations.
File-axis measuring letters and coordinate formats remain unchanged. Merged families
expose the optional detail fields below. No `_letter` hints are added.
Publication retains #70's coordinate-floor snap; a snapped letter names the positive file
axis, with coordinates, interval ends and end flags adjusted to that sense.

For a converted record, `origin` is a file-coordinate point: the recognition origin carried
into file coordinates. Scalar coordinates remain signed distances from this point along
their named measuring vectors. Reconstruct a point by adding each coordinate times its
vector to `origin`. Several directions share this one origin. Directions alone (such as a
fillet's `axis`, whose position is already `at`) do not require an origin.

| Measurement | Published measuring directions |
|---|---|
| Slot/pocket/channel `w_center`, `lo`/`hi`, `d_lo`/`d_hi` | `width_axis`, `long_axis`, new `depth_axis` |
| Riser `positions`, `other_positions`, `z_lo`/`z_hi` | `axis`, `other_axis`, new `height_axis` |
| Level `z`, `x_span`, `y_span` | new `height_axis`, `x_axis`, `y_axis` |
| Turning/prism stations, profile `span`, run intervals | the record's `axis` |
| Plate `u`/`v`, two-dimensional sections and flat `axis_line` | new ordered `section_axes` (the original two transverse measuring axes) |
| Pad `x0`/`x1`, `y0`/`y1`, `z0`/`z1` | new ordered `basis_axes` |
| Unequal ramp `half_widths` | new `width_axis`, low side first |
| Gusset support coordinates, mirror-plane coordinate | axis in the corresponding support/mirror pair |

Riser `vertical` means parallel to the published `height_axis`, not necessarily file Z.
`lo_at_envelope` and `hi_at_envelope` describe the low and high ends along `height_axis`;
they follow those endpoints when sense reverses. `body_levels` remains ascending along that
same published height direction. `open_sign`, `depth_sign`, pad direction
and gusset support-side signs refer to their published directions. Empty position arrays
remain empty. Nested levels and pattern members use the same decoding rules independently.
An unconverted record without `origin` retains the existing absolute file-coordinate rules.
Flat `axis_line` gives the two coordinates along `section_axes`. Start at
`q = origin + axis_line[0] * section_axes[0] + axis_line[1] * section_axes[1]`, then move
along `axis` until `dot(q - origin, axis_direction) = 0`. This also handles a stock cylinder
slightly inclined to its principal measuring axis. The resulting point lies on the stock axis.
Flat `stock_span` already uses `axis_direction` from the file origin and retains that rule.

A converted `body_key` becomes `{origin, axes, bounds, volume, area}`. `axes` is an ordered
three-direction basis; `bounds` is three `[lo,hi]` distance intervals from this origin.
A converted turned-profile `body_bounds` uses `{origin, axes, bounds}`. These preserve the
oriented box, not its larger file-aligned envelope. Unconverted box arrays retain their
old layout. `profile.axis_origin` is a full file-coordinate point after conversion.

Already explicit passage/recess frames remain self-contained. If independently rounded
reanchoring would invalidate a previously proved recess, carry its original relative geometry
and complete measuring frame without a second rounding; do not discard it or require provenance.

## Merged families and IDs

Publication merges `oriented_slots` into `slots`, `oriented_chamfers` into `chamfers`,
and `oriented_slot_patterns` into `slot_patterns`. Recognisers, their proofs, frame
voting and accepted occurrences are unchanged. No patterns are rediscovered and no
occurrences are deduplicated. Other families, including oblique through steps and blends,
remain separate.

| Source record | Published mapping |
|---|---|
| Oriented slot | `Slot`: `width_direction` → vector `width_axis`, `long_direction` → vector `long_axis`; `source.frame.run` → `depth_axis`; `source.frame.origin` → `origin`. Project source profile vertices into width/long directions for `w_center` and `lo`/`hi`; `source.run_interval` supplies `d_lo`/`d_hi`. Keep intrinsic `width`, `length`, and `body_key`. |
| Slot detail | Optional `source` retains the proved passage geometry; readers accept it absent or null. Plain slots currently publish null source. Unproved `end_radius` and `corner_radius` are null. No rectangular source is fabricated. |
| Oriented chamfer | `Chamfer`: `run` → vector `axis`, `turned:false`; retain `leg1_direction`, `leg2_direction`, `length`, `support_spans`, and `body_key`. Plain chamfers have null for these optional, unproved details. Support spans retain their distances along the run from `at`. |
| Oriented slot pattern | `SlotArray` or `SlotGrid`, with members mapped by the same slot conversion. Grids add explicit `row_direction` and `col_direction` vectors; the precision rule below applies to plain slot arrays and grids. |

Each family contains accepted plain records first, then accepted oriented records, in
each recogniser's existing deterministic order. IDs are `<merged-family>/<index>`.
Promoted measuring axes are normalised to unit length; existing detailed source vectors
retain their schema-6 precision. File-axis letters stay letters.
The append offset is the number of plain records. Pattern members, fingerprint IDs,
evidence status and face-accounting references use this same translation.

## Fingerprints and saved references

Fingerprints remain `quiddity-rust/fingerprint/3`. Their values change for merged
records because they include family and ID; oriented chamfers acquire the common
`turned:false` trait and oriented slots drop source-end proof traits. Optional detailed
geometry is not a new feature kind or a semantic trait. The slot-pattern precision
correction below also uses measured axes and pitches in intrinsic fingerprints.

For references saved from 0.1.1's schema-5 `oriented_*` records, use the merged family
and sorted defining faces, or run `quiddity correspond old.json new.json` on the saved
CLI recognition output and the new recognition (or STEP). Both use v3 fingerprints;
correspondence maps the former family semantics while retaining the old IDs on the
old side. Three slots, their pattern and the captured 0.1.1 oriented chamfer are pinned
by the legacy correspondence regression. Other fingerprint formats require
re-recognition. A bare recognition document without fingerprints must be paired
through its source STEP or defining faces.

Numeric IDs are run-local: do not persist a renamed family with its old numeric
suffix. Move saved selections and Specify `family:faces` overrides by defining faces
or correspondence's old/new ID pairs. Face indices are specific to a STEP revision;
use correspondence when the source revision changes.

Rust consumers obtain the merged contract through `PublishedFeatures::families()` or
serialization. The public `Deref` into the working inventory is removed so typed
field access cannot expose an unmerged family; working inventory access remains
internal. The published registry has 45 families; raw recogniser APIs retain their
internal types. Intrinsic fingerprints use exact-motion working measurements, with
axes resolved at extraction; no provenance-frame repair is needed by correspondence.

## Sealed interior voids and holes

A cylindrical cavity on a closed interior shell is an `interior_voids` feature,
not a hole. The hole recognizer splits each solid into edge-connected shells and
rejects cylinders on closed inward-facing shells (negative signed volume) before
building hole stacks. This does not depend on the interior-void sampler's grid
resolution. Such cylinders produce no `holes`, `hole_patterns`, or hole-derived
records. Their complete skin, including closing faces, remains accounted to the
`interior_voids` record. Both recognizers share this shell proof: every sealed shell
produces a void record even when the air-sampling grid cannot resolve it.

Sealed records use `volume_method: "closed_shell"` and the positive volume from
the shell's oriented boundary integral in `estimated_volume`. They do not use grid
samples: `grid_pitch`, `enclosed_samples` and `air_samples` are zero. Open chambers
retain `six_axis_grid` and their existing sampled measurements. Sealed void record
parameters and fingerprints therefore change to describe the shell measurement.
The complete void claim supersedes generic cylindrical-profile records on its skin,
as other recognized features already do.

Blind holes, through holes, and holes opening into pockets connected to the outside
remain on the outer shell and retain their existing recognition. Interior chambers
with an opening can still be `interior_voids`; the presence of that record alone
does not suppress holes. No schema change is required.

## Spotfaces and counterbores (For #118)

A near-side cylindrical step is a `spotface` only when its depth is at most 1.0 mm
and strictly below 0.2 times its diameter; otherwise it is counterbore-class. The absolute
limit is in quiddity's internal millimetres, like ADR 0008's other minimum-evidence
thresholds: a spotface only cleans up a surface; deeper recesses are functional.
Both comparisons use the published measurements, including depth rounded to two
decimal places, preserving the ratio test's existing rounding policy. Thus a raw
1.004 mm depth publishes as 1.00 mm and can be a spotface; 1.006 mm publishes as
1.01 mm and is a counterbore. Depth exactly 1.0 mm qualifies only below the ratio
limit; equality at the ratio limit is a counterbore.

The innermost counterbore-class step (nearest the bore) is `cbore`. Larger outer
counterbore-class steps are retained in optional `outer_steps`, outermost first;
no counterbore-class step or its defining cylinder faces is dropped. A qualifying
outer spotface remains in `spotface`, preserving spotface-plus-counterbore stacks.
Hole-pattern members carry the same fields, and pattern grouping includes outer steps.
Raised collars remain bosses, not additional hole steps.

| Hole field | Definition |
|---|---|
| `cbore` | `{diameter, depth}` of the innermost counterbore-class step, or null. |
| `spotface` | `{diameter, depth}` of the outermost qualifying spotface, or null. |
| `outer_steps` | Optional nonempty list of `{diameter, depth}` for the remaining counterbore-class steps, outermost first. Absent when there are none; never emitted as an empty list. Old records without this field deserialize as no outer steps. |

Every step depth, including `cbore`, `spotface`, and each `outer_steps` item, is
its own cylindrical mouth-to-floor axial span in millimetres, rounded to two
places. It is not cumulative from the hole opening. For example, outer diameter
60 x depth 5 followed by diameter 18 x depth 6 publishes `outer_steps: [{diameter:
60, depth: 5}]` and `cbore: {diameter: 18, depth: 6}`; the inner floor is 11 mm
from the hole opening. Identical step readings at interchangeable mouths retain
both sets of defining faces; differing readings are omitted, as for counterbores.

Recognition schema 6 and fingerprint format 3 are unchanged; `outer_steps` is an
additive optional field. Re-recognise affected parts. Like counterbores and
spotfaces, outer steps describe machining geometry and belong in hole fingerprints:
`outer_steps.count` records their number, and `outer_steps.<i>.diameter` and
`outer_steps.<i>.depth` retain paired sizes in outermost-first order. These keys
are absent when there are no outer steps, so unaffected fingerprints are unchanged.
Affected fingerprints change within v3 through these sizes and the existing
spotface/counterbore traits. Rust callers constructing `HoleRecord` literals must
supply `outer_steps` (normally `Vec::new()`).

## Closed blind-recess floor claims (#205, #207)

Accepted flat-bottomed blind holes and blind `pockets`/`prismatic_pockets` also
claim an already-consulted planar floor perpendicular to the inward depth
direction, when every boundary edge meets their own deepest walls. A larger
plane, or a floor continuing into a slot or step, remains context. One shared
rule completes these claims after reconciliation and the existing completion
stages; record parameters are retained. Derived patterns follow their members and
therefore include the members' completed floor claims.

Whole-body classifications `thin_wall_bodies`, `sheet_metal_bodies`, `plates`,
`polygonal_stock` and `turned_bodies` can coexist with the new recess claim.
Evidence families `step_levels` and `risers` are not individual manufacturing
features, as in draftwright: they may co-own a floor with the feature that closes it.
No other family is a co-owner exception. Existing manufacturing claims take
precedence; competing new individual records use first-owner priority: holes,
pockets, then prismatic pockets, even when their walls are identical.
Section-recess claims are retained; exact duplicates carry the optional `source`
reference described below (#206). See [face accounting](face-accounting.md) for
ownership precedence and the distinction between discovery evidence and
completed claims.

## Slot precision correction (for #92)

The plain recess recogniser retains its legacy two-decimal positions, for stable
recognition and file-axis output. `Slot::published` in `recess_records.rs` applies
that grid after reductions have measured the raw spans; channels apply the same
position grid in `recess_core.rs`. These rounded values are unsuitable as distances
in a turned measuring basis. Converted slots now publish their retained raw
`w_center`, `lo`, `hi`, `d_lo` and `d_hi`; these distances are not independently
rounded to a fictitious file grid. The measuring basis still uses #70's
coordinate-floor snap. File-axis extents keep their existing publication rule.
Pockets and channels are outside this correction.

Slot patterns retain exactly their recognised members and grid cells. Publication
remeasures arrays and grids from the raw member centres, before any position
rounding, and snaps explicit directions to a file axis only within the existing
coordinate-floor movement bound. Array sense runs from the endpoint with the
lexicographically greater defining-face list to the other endpoint; reverse the
member list together when necessary. This makes sense stable under rigid moves
that preserve face identity. Grid pitches and centre come from those same raw
centres. File-axis pattern geometry is also remeasured, so a copy that happens to
land on file axes does not retain a rounding-induced skew. Feature IDs, proofs,
inventory, schema 6, fingerprint version 3 and the crate version are unchanged.


## Annular face grooves

The optional `annular_grooves` family contains `AnnularGroove` records and is
omitted when empty. This additive family keeps `quiddity-rust/recognition/6`;
existing hole and boss records are kept and marked with `superseded_by` when
built from an accepted groove's walls. It describes a blind annular face recess
of rectangular radial section, such as a face-seal housing. It makes no nominal
O-ring size or intended-function claim.

Each record publishes `axis` (a unit vector from the floor towards the mouth),
`floor_center` (a point in file coordinates), `inner_radius`, `outer_radius`, and `depth`
(in millimetres). All floor pieces and both cylindrical walls are defining faces; planar faces
at the two mouth rims are consulted evidence. All must belong to one strictly valid solid.
The two complete circular floor boundaries must be concentric, their cylindrical walls
coaxial, the floor junctions concave, and the mouth junctions convex. Both walls must end
at the same open plane. Interrupted walls, treated rims, unequal depths, invalid ownership,
and competing defining claims are refused. External turned bands remain `grooves`.


The annular floor may be partitioned into native planar faces joined smoothly on the same
plane. Internal edges cancel, leaving exactly two nonbranching closed boundary cycles.
A rim may contain several circular edges, but their centres/radii must agree and their
actual angular intervals must cover a complete turn. Counting overlapping arc lengths
as a full rim is refused. The cylindrical walls must still each be complete; every floor
piece is retained in the defining claim and strict ownership proof.

## Additive duplicate references (#204, #206)

Optional `superseded_by` on `holes` and `bosses` names the accepted annular groove
whose cylindrical walls define that legacy record, for example `"annular_grooves/0"`.
Only records defined entirely by that groove's walls are marked. A refused groove
adds no marker. Measurements, defining claims, evidence, fingerprints, record order
and IDs are retained, including the legacy hole's `bottom` interpretation.

Optional `source` on `section_recesses` names a specific accepted source record
with exactly the same final defining-face set, for example `"pockets/2"`. The source
families are `channels`, `edge_open_circular_pockets`, `edge_open_prismatic_recesses`,
`pockets`, `prismatic_pockets`, `rectangular_blind_slots`, `round_bottom_blind_slots`
and `section_passages`. Face order is irrelevant; partial overlaps and union coverage
are insufficient. Recess-only records have no marker. Multiple accepted source
records can describe the same face set: `source` remains a single reference, chosen
by lexicographic family name then lowest record index. It names an identical record,
not exclusive provenance or a claim that the recess projector used that record.

Both fields are additive, omitted when absent, and use schema 6's `family/index`
run-local reference form. No records are removed, reordered or renumbered, and
`section_recess_patterns` and their member indices are unchanged. Consumers may
follow the references to show one feature or exclude marked duplicates from feature
counts and face attribution; existing consumers can continue reading every record.
Drawings that obtain recess geometry from `section_recesses` can continue doing so.
Saved rank-based references remain valid, and face-based answers retain their claims.
The MFInstSeg scorer honours only `superseded_by`. It retains `source`-marked
section recesses: predicted faces are already a set per class, so duplicates in the
same class add nothing, while a class may map the recess without mapping its source
family (notably rectangular blind slots, class 17). The `source` marker is for
consumers, who may drop duplicates; it is not a scorer filter. Class mappings and
selectors are unchanged. This is an additive schema-6 change and requires no
breaking release.

## Face grooves (for #123)

The optional `face_grooves` family publishes one `FaceGroove` per annular recess in a
planar face. Congruent recesses remain separate, following `annular_grooves`; no pattern
or count is inferred. The wall surfaces establish the axis intrinsically, in any direction.
There is no frame, turned-profile or turned-body eligibility gate. A recess in a prismatic
host counts when its walls prove the same coaxial geometry.

The record fields are `axis` (unit vector pointing from floor to mouth), `axis_point`
(mouth centre in file coordinates), `outer_diameter`, `inner_diameter` (both at the actual
rim on the mouth plane), `depth` (mouth plane to floor), `outer_wall_angle`,
`inner_wall_angle` (cone semi-angles in degrees to the axis; square cylindrical walls are
reported as 90°), `floor` (sorted planar floor faces), and `faces` (all defining faces).
The source measurements are retained without nominal-size or angle rounding.

A floor is one plane square to the common wall axis, or a smoothly connected coplanar
region. Internal split edges cancel; its boundary must be exactly two complete concentric
circular cycles. Split circular edges must cover the circle once, without overlaps or gaps.
Each wall is a complete cylinder or cone, possibly a chain of full coaxial bands. Both
walls must rise towards the same open face plane, where the circular rims meet planar
lands facing outwards. These lands can be disconnected faces of that plane. The opening
must remain annular; walls may lean under the rim. No machining-access restriction is
inferred.

Tangent coaxial toroidal floor and rim rounds are included in `faces`, provided the
wall/floor proof holds. Each round must occupy less than a meridian half turn and stay
between its endpoint axial levels; a radial extremum between the endpoints is allowed.
Conical floor/rim chamfers are included too. For a treated
wall, the cylinder/cone segment with the greatest axial extent supplies its wall angle;
shorter segments are treatments, and equal longest segments must agree. A round alone
cannot substitute for a wall. The entire floor, walls, treatments and consulted mouth
lands must have strict evidence on one valid solid. Partial/interrupted walls, extra floor
openings, noncoaxial boundaries, unequal mouth planes and competing candidates are refused.
A counterbore has no rising inner wall and remains a hole; radial grooves on cylindrical
stock remain `grooves`.

Publication uses the completed defining claims after existing reconciliation, section
recesses and residual cylindrical profiles. Any defining-face overlap with another
feature or reserved refusal suppresses the whole new occurrence; body classifications
keep co-ownership.
In particular, square-walled recesses already recognised as `annular_grooves` retain
their existing records. This additive family changes no existing feature or fingerprint.

Fingerprint format 3 uses the two diameters, depth and two wall angles as intrinsic sizes,
`axis` as direction and `axis_point` as placement. `floor` and `faces` are source indices,
ignored as parameter sizes; the usual defining-face list, centroid and neighbourhood
provide the fingerprint's geometric evidence. Recognition remains schema 6.

The local 608ZZ reproduction has floors 4 and 11 and walls 2/6 and 9/13. Its two
records measure ⌀18.5/⌀13 at the rim, depth 0.25, with cone angles
44.999971914041566° (approximately 45°). At the floor, the measured diameters are
approximately ⌀18.00000049/⌀13.49999951. The reproduction is not redistributed.

## Evaluation mappings

`lip_bevelled_blind_slot` selects an existing complete `edge_open_recess` geometry:
exactly two parallel straight sides, a flat floor and one planar lip bevel flaring
outward into the open mouth. Its five profile vertices and normal planar run-end
surfaces must prove exactly one capped and one open end. The run must be strictly longer
than both nominal width and depth beyond coordinate/interval rounding uncertainty;
ambiguous run/depth roles remain excluded. `coordinate_decimals` and `interval_decimals`
are the published profile-point and run-station precision (4 and 3). The existing
section-recess recogniser proves the swept region and solid. The mapping preserves
its defining faces, including the bevel, and does not add consulted cap faces.

`compatible_polygon` is a conservative scoring predicate over the published section
coordinates. `shape` is `triangle`, `rectangle`, or `regular_hexagon`; `decimals` must
be the field's publication precision (4 for section-passage boundary points, 3 for
prismatic-pocket corners). A rectangle has four right angles; a regular hexagon has
six equal edges and sixty-degree exterior turns. The polygon must be straight and
strictly convex at that coordinate resolution. Dot/cross and edge-length tolerances
come from half-grid coordinate rounding, not a model or dataset scale. Either winding
and arbitrary section orientation are supported.

A complete alternative canonical shape is excluded. Curved profiles, clipped corners,
intersections, irregular polygons and unresolved edges remain ambiguous and retain the
old correspondence; raw wall/vertex counts do not identify their original cut shape.
Likewise an unknown hole end is retained for blind-hole scoring, while a proved through
end is excluded. Pocket flags exclude only a proved incompatible corner or curved-end
subtype. A missing flag does not prove the opposite subtype. These predicates never
read labels, model identity, topology or recognition internals beyond published records.

The round scoring correspondence also includes complete rolling-ball `blends` selected
by the published material side (`convex` or `concave`). MFInstSeg round includes both
external rounds and internal fillets, as confirmed by its labelled concave blend faces.
Their smooth supports and complete path are already proved; this also includes external
rounds below the legacy fillet radius floor.
The fillet recogniser's radius and convexity rules are unchanged. Defining-face sets
shared by a fillet and a blend still count as one prediction.

The MFInstSeg scorer evaluates the internal caller-space inventory. Its chamfer class
therefore names both `chamfers` and `oriented_chamfers`, which this document already
publishes together as `chamfers`. This correspondence changes scoring only: a complete
oriented bevel is already proved by its recogniser and its defining face remains the
claim. Identical face sets across either source count once.

`tests/fixtures/mfinstseg_mapping.json` schema 2 permits per-family `selectors`. Each
selector names a JSON-pointer `field` in the published record, its comparison, and its
geometric `evidence`. All predicates on a family must match before its defining-face set
is passed to the scorer. Record indices stay paired with their defining faces, and labels
never select predictions. The circular-through-slot mapping selects the existing
`section_recesses` classification `feature_kind: channel`, `section_shape: circular`:
these are open cylindrical troughs with both run ends proved open. This changes evaluation
correspondence only, not the recognition schema or the geometry engine.


## V-section through channels

The optional `v_channels` family contains `VChannel` records, omitted when empty.
It keeps recognition schema 6 and does not change any other family's records.
`axis` is the unit run vector (its sign is a presentation gauge), `opening` is the outward
unit normal to the lip plane, and `at` is the trough edge's midpoint in file coordinates.
`length`, `width`, and `depth` are millimetres; `included_angle_degrees` is the angle
between the two sides, without any preferred angle or dimensional ratio.

The defining faces are two complete rectangular planes meeting concavely at the trough.
Their opposite lips must be coplanar and their ends aligned. Each lip and each end must
meet an outward-facing exterior plane convexly. The apex must lie between the lips when
projected onto the mouth; interrupted walls, missing ownership, and competing wall claims
are refused. All defining and consulted faces require one strictly valid solid. A pair of
walls already contained in a proved closed section passage is part of that complete
passage, not a separate open channel. This exclusion leaves passage recognition intact.


## Semicircular-bottom blind slots

The optional `semicircular_blind_slots` family contains `SemicircularBlindSlot` records,
omitted when empty, within recognition schema 6. Its section has one semicircular bottom
and two tangent straight sides. This differs from `round_bottom_blind_slots`, whose
section has two quarter-cylinders and a flat floor; that existing family is unchanged.

`axis` points from the open run end towards the blind cap; `opening` points out of the
lateral mouth. Both are unit vectors in file coordinates. `at` is the section bounding
box's centre halfway along the run. `length`, `radius`, and `straight_height` are
millimetres: section width is twice the radius and depth is radius plus straight height.
The cylinder, both straight sides, and the planar blind cap are defining faces. The
convex planar faces at the run mouth and lateral opening are consulted evidence.

The complete cylinder boundary must have two semicircular rims and two straight run
edges; the rectangular sides must be tangent to it and rise equally towards the lateral
mouth. The blind cap has exactly that U profile and meets all three swept surfaces
concavely. Both mouth boundaries meet outward-facing planar context convexly. Every
source face must belong to one strictly valid solid. Interrupted or treated boundaries,
missing caps/ownership, and competing defining claims are refused. The cylinder axis
and unique cap establish run/depth roles without a dimensional preference.

A trough may instead be rounded: one concave cylinder along the run whose two straight edges
each meet a flank tangentially, its two ends opening convexly onto exterior planes like the
flanks'. The round is consulted, not defining; it keeps its own `blends` record, as a slot's
corner rounds do. `trough_radius` is then present. `at` and `depth` refer to the flanks'
intersection, the virtual sharp trough the round replaces, so a rounded and a sharp V of one
section publish the same `depth`, `width` and angle. The round must be tangent on the lip side
of that intersection for both flanks. A sharp trough omits `trough_radius`, so its record and
fingerprint are unchanged. On NIST CTC-01 the two V notches through the plate edge (flanks
11/13 and 15/17, R10 troughs 12 and 16) are such channels along the plate's thickness.

## Rectangular through-notches

The optional `rectangular_through_notches` family contains `RectangularThroughNotch`
records, omitted when empty. It keeps recognition schema 6 and fingerprint format 3.
A notch is a U cut through a recognised plate from its edge: a bottom wall and two parallel
side walls facing each other, all planar and perpendicular to the plate axis. `axis` is the
plate's thickness direction (its sign is a gauge), `opening` the bottom wall's outward normal,
out through the mouth, and `at` the centre of the bottom wall, midway between the sides and
through the thickness, in file coordinates. `width` separates the side walls, `depth` runs
from the bottom wall to the mouth plane and `thickness` is the plate's; all are millimetres.
`corner_radius` is the radius of the two concave corner rounds, or null for sharp corners.

The defining faces are the bottom and both side walls. Each is one loop of four lines: two
along the plate axis spanning exactly the plate's raw `lo…hi`, and two shared convexly with the
plate's own faces at **both** slab ends, as edges of those faces' outer loops. The bottom meets
each side either across a concave line or through one concave cylinder along the axis, tangent
to both and spanning the slab; the two corners are both sharp or both rounded with one radius.
Each side wall's other axial edge is the mouth: a convex edge with a planar wall facing the
bottom's way, and both mouths lie in one plane beyond the bottom. Corner rounds keep their own
`blends` records, as a rectangular slot's do; they, the mouth walls and the plate faces are
consulted. Defining and consulted faces require one strictly valid solid.

These conditions separate the notch from its neighbours without any dimension or ratio: a
closed slot's walls have no mouth and lie on inner loops of the plate faces; an edge-open
recess's walls stop at its floor rather than the far slab face; the waist between two thicker
end blocks has side walls taller than the slab (they belong to the blocks, and face
accounting gives them `stock_wall` evidence). A wall pair a full-span floored channel defines
belongs to that channel, which is how a notch through a part's whole thickness is already
published; a plate needs the body to extend beyond it along its axis. A face proposed by two
notches defines neither, and a notch is dropped with its plate when reconciliation rejects the
plate. On NIST CTC-01 the two 85 × 25 R10 notches in the +y plate edge are
`rectangular_through_notches/0` (faces 31, 33, 35) and `/1` (37, 39, 41).

## Polygonal boss caps

A polygonal boss's record also claims its free-end cap, the face its proof already
consulted, after reconciliation: a plane facing along the boss axis at the record's `base` or
`top`, lying within the side ring's transverse extent. A support face or any wider face the
ring ends on is not claimed. Feature selection and record fields are unchanged; the boss's
fingerprint `faces`, area-weighted `position` and `neighbourhood` change within format 3.

## Straight convex fillets (for #120)

A partial cylinder rounding a convex edge between two tangent planar walls is a fillet
at any angle and in any direction. Principal and oblique runs use the same intrinsic
proof: two distinct nonparallel transverse walls, each tangent within the existing
coordinate tolerance, an outward cylindrical material side, and an empty-space probe
towards the round from the virtual sharp corner. Radius limits are unchanged.
The contact tolerance is `max(part.file_uncertainty(), COORD_FLOOR)`, with
`COORD_FLOOR` (0.000001 mm) as the fallback for files without a stated precision.
The STEP reader supplies the file uncertainty in millimetres; CTC-03 states 0.00002 mm.

The cylindrical sweep must be greater than zero and at most 135° and match 180° minus
the interior wall angle (the turn between outward wall normals), within 0.5°. This rejects
grossly truncated bend skins without demanding exact agreement from modelled geometry:
CTC-03's 90.0246° round passes; sm-hanger's 25.4° skins between perpendicular walls do not.
This replaces a fixed
extent cap: acute corners can sweep over 90°, while obtuse corners sweep less. The 135°
cap keeps wall angles at least 45°: a near-half-turn between nearly parallel walls is a
full-round groove/slot bottom, not an edge fillet. CGB202's R90 face 2074 stays a groove.
Only planes tangent within the existing length tolerance participate in the corner query.
Non-tangent transverse end/cut planes are ignored; the competing-corner guard still rejects
three distinct tangent supports or distinct parallel tangent walls, while merging coplanar
splits. This preserves NIST CTC-02 AP242's six R10, 82° rounds with a third non-tangent plane.
Bores, parallel supports, competing tangent walls and concave rounds remain excluded. The turned-torus
fillet proof and extent limit are unchanged. Chamfers retain their right-angle contract.

The internal untagged `FilletAxis` stores a principal letter for every run passing the
existing `principal_axis` / `AXIS_ALIGNED_COS` band, including its legacy approximation.
Only runs outside that band use a full-precision unit vector, with its dominant component
positive in the working frame. Principal records
retain their serialization and sort order; oblique records sort afterwards by direction
and anchor. Publication rotates vector axes directly into file coordinates. Fingerprint
format 3 already reads both letters and vectors as axes and retains the exact direction.
This fits schema 6's existing axis contract, so neither version changes. Accepted fillets
supersede generic blends on their defining faces; family IDs, fingerprints and face
accounting references change accordingly. Move saved selections by defining faces or
correspondence as described above.

Whole-body classifications may retain the same faces as individual features. In
particular, a round becoming a fillet retains its `thin_wall_bodies` or
`sheet_metal_bodies` classification, just as existing fillets, holes and blends already
do. This co-ownership does not replace the round's geometric proof or permit a second
claim by another individual feature family, except for the explicitly marked legacy
duplicates described above.

Sheet-metal bend skins must satisfy the same edge-round proof to be fillets. The old
axis-aligned path skipped tangency and could label a skin cut short of its tangent wall
as a fillet. For example, sm-hanger's R7 faces 73 and 76 remain the outer skins of
sheet-metal bends 6 and 7 (inner R3, face pairs 73/75 and 76/85), but are no longer
fillets: each has a transverse wall 4 mm away from tangency and sweeps only 25.38°
between perpendicular walls.
CTC-05's R203.2 faces 14, 15, 168 and 169 also fail tangency, by 2.54 and 133.35 mm;
they are cylindrical profile faces under the residual rule for #127 below.

Individual features reserve their accepted defining faces before fillets. The inventory
uses its `kept` evidence and reconciliation dispositions, so rejected proposals do not
reserve faces and a suppressed fillet cannot suppress its generic blend. Whole-body
classifications and generic surface fallbacks use shared claims; derived patterns and
section recesses retain their existing publication paths.


## Cylindrical profile faces (for #127)

| Family | Record type | Parameters |
|---|---|---|
| `cylindrical_profiles` | `CylindricalProfile` | `radius` (mm), `axis` (principal letter or unit vector), `axis_point` (file-coordinate point on the axis), `side` (`convex` / `concave`), `sweep` (radians), `faces` (sorted defining face indices) |

The name describes a cylindrical portion of the part's profile without inferring a
machining operation. Native cylindrical surfaces are eligible in any direction,
convex or concave, with strictly positive angular coverage strictly below pi radians.
There is no frame-axis gate. Connected, coaxial, equal-radius patches of the same
oriented surface and solid form one record. Their sweep is the union of their angular
intervals, so axial splits count once. The complete connected surface must be below
180° before any claimed patches are removed; a bore split into narrower patches stays
excluded. Disconnected congruent faces remain separate occurrences.

This residual projection runs after reconciliation, section-recess projection and the
completion of published defining claims. Only unclaimed faces are eligible; rejected
proposals do not reserve faces. Existing blends and all other individual feature
claims take precedence. Whole-body `thin_wall_bodies`, `sheet_metal_bodies`, `plates`
and `polygonal_stock` may retain co-ownership. Refused evidence is also excluded.
The existing cylindrical fillet proof is applied without its reporting radius limits;
any face it accepts remains an edge round rather than becoming a profile. No fillet
or blend predicate changes. Like projected section recesses, this family appears in
completed recognition, rather than the inventory's physical candidates.

Concave faces with radius strictly below `min(2 mm, 0.05 * smallest_box_dimension)`
are excluded for the separate inside-round stream. Equality is eligible; convex
faces have no radius cutoff. The box is the run's part box. This is an explicit split,
not proof that larger concave profiles cannot later be identified more specifically.

`radius` and `axis_point` follow the existing three-decimal measurement/position
conventions. The axis point is at the middle of the defining faces' axial extent.
Only exact principal runs use letters in discovery; oblique runs retain their unit
vector. Publication carries both axis and point into file coordinates using the
existing schema-6 axis contract. The optional family is absent when empty.
Fingerprint format 3 uses radius and sweep as sizes, side as a trait, axis as axis,
and axis point as placement. Face indices are ignored as intrinsic fields; the
occurrence's defining faces supply the fingerprint faces, position and neighbourhood.

CTC-05 faces 14, 15, 168 and 169 are four separate convex R203.2 mm profile surfaces,
not fillets: their walls fail tangency. Existing records keep their IDs and parameters.

## Turned bodies (For #123)

By default, optional `turned_bodies` publishes one `TurnedBody` for each valid solid
whose entire boundary consists of analytic surfaces of revolution about one common
line. The proved stock-flat exception in the #157 section below extends this rule. A plane
must be square to that line; cylinders, cones and tori must have that axis; sphere
centres must lie on it (their parameter polar axes are immaterial). NURBS planes also qualify when positive weights and the complete control net
certify the support inside the same plane-precision slab. A sampled fit alone
cannot establish a plane. Other unsupported surfaces, keyways, cross holes and
offset/second revolution axes refuse the whole solid. Flats require the proved
interruption exception below. There is no principal-frame or number-of-diameters gate. The linear
precision is `max(file_uncertainty, COORD_FLOOR)`; parallelism is tested as the sine
of the axis angle times the body's largest intrinsic directional extent (or support
radius, whichever is greater), against that same precision. Axis-line offsets use
that linear precision. No recognition-policy angular tolerance is used. A complete
sphere admits every axis through its centre; its native polar axis supplies a
conventional representative that follows rigid motion, without limiting eligibility.

`axis` is a positive principal letter or a unit vector, and `axis_point` is the
midpoint of the axial extent on that line, published on the 1e-8 mm point grid.
For an uninterrupted body, `od` is the maximum transverse extent (including
spherical/toroidal extrema); interrupted stock uses its nominal band diameter as
described below. `length` is the exact axial extent. `bores` lists internal cylinder diameters and
`span: [lo,hi]`, signed distances from `axis_point` along `axis`; touching axial
splits of the same diameter merge within model precision. Diameters come from the
analytic support, and disconnected axial spans remain separate.

`profile` is sorted by axial start, end and radius on the existing 1e-8 mm point
ordering grid, then face. Measurements themselves stay exact; the ordering grid
prevents round-off at a shared station from permuting its segments on a write/read
round trip. Each segment gives its
`face`, `kind` (`cylinder`, `cone`, `torus`, `sphere`, `plane_shoulder`), `side`
(`external`, `internal`, or `shoulder`), and `start`/`end` coordinates `[axial,radius]`
in the same midpoint-based measuring system. For curved meridians, `arc_center`,
`arc_radius` and signed `arc_sweep` describe the exact circular arc, positive from
+axial towards +radius; otherwise these fields are null. This includes internal
and reentrant profile surfaces as well as the outside, without creating recess
feature records. Face splits and torus axial turning points remain separate
segments; each curved segment is monotone in axial position. Together these describe a
half-section, including edge chamfers and plane shoulders.

`body_key` follows existing body classifications. Optional `profile_key` reuses
`turned::TurnedProfileKey` and `profile_key_from_bands` over external cylinder bands
when the intrinsic axis is principal, joining existing turned-step/groove records.
Its fields follow the existing turned-profile publication contract, under
`profile_key` instead of `profile`. Any-axis bodies use the explicit meridian
without requiring that legacy principal-axis join. `ProfileAxis` and `Placed` are
also reused. All faces co-own `turned_bodies/N` and their feature records; body
claims do not reserve faces against features or residual cylindrical profiles.

After all existing decisions and projections, suppress an accepted cylindrical
boss only when its defining cylinders belong to that turned body, each raw
diameter equals its OD within model precision, its raw axial span covers at least
90% of the body length, and each end gap is at most 5% of that length plus model
precision. This deliberately permits small end chamfers. Shorter/smaller end
bosses remain. No other record is removed or changed.

Fingerprints remain `/3`: axis is an axis, OD/length/bore diameters and meridian
measurements are sizes, surface kinds and material sides are traits. Meridian
sizes are canonicalised together over both axis senses, retaining station/radius
associations. Points, body keys and the legacy profile join are placement;
profile face indices are ignored in parameters and remain defining evidence.

## Boss ends and trimmed plates (for #134)

A boss's `height` is its complete axial extent, including adjoining proved end
chamfers and edge rounds. Its `location` is the axis point at that complete free
end; `axis` still points from base to free end. Both ends can contribute. Coaxial
cones/tori must be proved edge treatments beyond the cylindrical band, meeting the
cylinder at a convex junction at that end. A tangent toroidal/spherical round must
have convex material curvature; a concave root fillet into the host never lengthens
a boss. Each treatment's axial and radial legs must be no larger than the smaller
of the boss radius and half its straight cylindrical length. This makes the end
break subordinate to its own boss in both directions, independent of the whole
part's size, and excludes long tapers. A treatment eligible for two cylindrical
segments extends neither: no treatment span is counted into two bosses. A tangent
spherical end round contributes only when its equator meets the cylinder end.
The cylinder faces remain defining evidence; treatments are consulted context and
can keep their own feature records. A genuinely shorter shaft still has a shorter
height. No cylinder-only length field is added: no existing consumer requires it.

Plate coverage measures the supporting skin: its planar area plus the signed
orthogonal projection of adjacent proved convex chamfers/rounds lying inward of
that real skin plane. Projection measures the removed strips, unlike summing the
larger curved/sloping surface areas or using a bounding rectangle that fills holes.
Each strip is counted once per side. The existing area-fraction, thinness and
coordinate-band thresholds are unchanged. Two real opposed planar skins are still
required, so removing a skin removes the plate and thinning it changes `hi - lo`.
The plate's transverse `u`/`v` centre includes those projected strips and their first
moments; `lo`/`hi` remain real supporting planes. Even a round below the standalone
fillet reporting floor trims its support and participates in this measurement.

Boss fingerprints use the measured axial midpoint (before height publication),
and plate fingerprints the area centre of the restored skins. Their neighbourhoods
contract proved convex edge treatments to the neighbouring supports, counting each
support once. Consequently adding an edge break carries the supporting boss and
plate, while the treatment and the physically trimmed faces retain their changes.
Within an aligned matched plate, mutually unique equally oriented skin planes seed
face correspondence even when trimming makes their areas differ substantially.
The global face-overlap rule and matching cutoffs are unchanged.

Schema `/6` and fingerprint format `/3` stay unchanged: these are corrected values
in existing fields, following the intentional-value-change rule used for #71 and
#92. Re-recognise saved affected fingerprints to obtain the corrected measurements.
Correspondence costs and changes read the corrected `sizes.height` and placements.
The evidence host lookup uses the complete `location - axis * height` base target
using physical boundary planes only; consulted context does not create hosts.
Treatment proofs used for extent work on oblique axes too, independently of
standalone treatment presentation. Turned fillets report their actual convex or
concave material side. Recognition and fingerprinting share the same Context,
reported treatment proofs, boss measurements and supporting-skin moments; plate
coverage and fingerprint placement use one projection/deduplication implementation.
Planar strips reuse planar area/centroid measurements. A cone's axial projection
has a constant normal component, so its surface moments also measure that strip;
the same moments are carried rigidly into its face fingerprint instead of being
integrated again. Other projection directions retain signed surface quadrature.
haecceity's kernel and PMI models do not consume `BossRecord.height`, and PMI face
anchors continue to refer to the original defining faces. No PMI schema changes.

## Incomplete recognition (For #161)

The design follows xu9299's proposal in Python quiddity [#806](https://github.com/pzfreo/quiddity-python/issues/806) and [PR #807](https://github.com/pzfreo/quiddity-python/pull/807).

Family isolation is always on in native builds with unwinding. If discovery or a
family post-pass panics, independent families still return results. Only incomplete
runs add these top-level fields to the `/6` document:

```json
{
  "failures": [
    {"family": "countersinks", "stage": "discover", "message": "example panic"},
    {"family": "hole_patterns", "stage": "discover", "blocked_by": ["countersinks", "holes"]},
    {"family": "holes", "stage": "discover", "blocked_by": ["countersinks"]}
  ],
  "complete": false
}
```

This excerpt omits the ordinary document fields and other blocked families. `failures`
is sorted by family, then stage; `blocked_by` is sorted. Each entry has `family`,
`stage` and exactly one of `message` or `blocked_by`. Family names are the internal
source names, before oriented-family merging (`section_passages` for passages).
An empty failed or blocked family means *not evaluated*, never "none found".
Panic payloads retain their string message (`&str` or `String`); other payloads use
`"non-string panic payload"`. Successful runs omit both fields and retain identical bytes.

The dependency map is countersinks → holes → hole patterns (whose members also carry
countersinks); passages → oriented slots and V channels; plates and channels → through
notches; thin walls and modelled-thread claims → freeform surfaces; step levels → risers;
and accepted holes, gusset ribs, slots, pockets and oriented slots → their patterns.
Section recesses depend on all eight recess source families. Residual cylindrical profiles
and face grooves depend on all specific accepted face claims and refusals, with face grooves
also depending on cylindrical profiles. OD-boss suppression consumes bosses and turned bodies.
Shared geometry caches are retryable `OnceLock`s, not family dependencies.

Stages are `discover`, `section_recess_projection`, `claim_drill_point`,
`claim_rectangular_end_walls`, `claim_terminal_cap`, `append`, or `suppress_od_bosses`.
A failed family contributes no candidates, defining faces, claims or refusals to
reconciliation/publication. A failed mutating post-pass discards its result and rebuilds
reconciliation with that family revoked before any later post-pass runs. Existing ordinary geometry refusals retain their
contract; they are distinct from panics.

STEP reading, context construction, reconciliation, document assembly and output validation
remain fatal/outside the family boundary. The library installs one panic-hook dispatcher:
a thread-local scope silences only caught family panics, while other threads and uncaught
panics reach the previously installed hook. Installing a new application hook later replaces
that policy. No hook is replaced in aborting builds. The recognition CLI retains its successful exit
status for a usable partial document and emits its existing-style warning on stderr;
`quiddity serve` and the Python wheel carry the same recognition fields automatically.
WASM builds abort on panic, so this isolation has no effect in the browser.

## Interrupted turning and proved corners (For #157, #159, #160)

Turned steps retain the OD-fill and square-section guards. A section flattened by
proved stock flats may bypass only the square-section guard: all other supports
must have one common revolution axis, the flats must be parallel to it at model
precision, and complete external coaxial cylindrical bands must cover more than
half the solid's axial length (overlaps counted once). A wholly D-section extrusion
or incidental cylinders on a prism do not establish this majority. Turned bodies
use the same exception; their meridian profile describes the revolved supports,
while the separate `flats` records describe the planar interruptions. Body claims
still include the entire boundary, including shoulders and flats; OD is the stock
diameter, including the interrupted bands.

Turned fillets on one valid solid merge arcs only when the raw torus centres,
axis lines, major/minor radii, material sides and meridian intervals agree at model
precision. One record retains a leader on an actual source arc and all constituent
arcs as its exact defining faces. Equal radii on different rings remain separate.
This uses the existing multi-face record contract and adds no fields or family. Historical fillet inventory floors continue to count constituent
torus patches (one per source arc), while cylindrical fillets including spherical
caps still count once. Grouping cannot satisfy a floor by dropping a patch. In the
cgb202 regression, 708 old occurrences become 542 callout records with the same
708 primary round patches; the historical floor remains 708.

Planar chamfer and angled-step `corner` is optional/null when unproved. Retention
requires consistency of the measured legs and angle with the corner/anchor points
at their published precision (0.001 mm for points and legs, 0.01 degrees for angles).
The point uncertainty is propagated into the transverse support basis and angle
interval; a fixed angular tolerance would reject correctly rounded short legs.
Rolled walls use their actual support directions. The volume proof uses unrounded
geometry and the common span of the two straight spring edges (including collinear
splits), excluding curved/oblique terminal extensions and attached caps. Where
that pair is unavailable the entire bevel-face span must pass. Its triangular prism is inset on every side by the modelling
precision plus the coordinate uncertainty of `2*at-corner` (sqrt(3) times 0.0015 mm).
Thus boundary contact is allowed, while any material volume in the remaining
interior refuses the corner; there is no allowed occupied-volume fraction. A
centroid and nearest support intersection alone are insufficient. Recognition
schema 6 and fingerprint format 3 stay unchanged.

## Helical coil springs

The optional `helical_springs` family publishes `HelicalSpring` records and is absent
when empty. This is additive: recognition schema `/6`, fingerprint format `/3` and
crate versions stay unchanged. The CLI, server and Python `recognise` JSON expose
the same family and document record; Python has no separate record-class API.

`axis` contains a unit `direction` and the point on its line nearest the file origin.
All coordinates are in the STEP file frame, including after part-frame recognition.
`start` and `end` bound the physical wire along that direction; `free_length` is their
difference. Millimetre measurements are `wire_diameter`, `mean_diameter`,
`outer_diameter = mean_diameter + wire_diameter`, `inner_diameter = mean_diameter -
wire_diameter`, and `pitch`. `hand` is `left` or `right`, independent of axis sign.
`total_coils` measures the centreline's angular sweep in turns, including the short
straight terminal extensions of segmented models (`total_coils_basis` states this).
Thus a nominal 6.5-turn construction with extended cylinder tips can measure 6.537
turns. These are measured dimensions, not a nominal spring designation. A segmented
solid's protruding chord ends can exceed the fitted outer diameter slightly.

Recognition currently establishes constant-pitch, plain round-wire ends.
`end_type` is `plain`; no closed/ground-end deduction is made. `active_coils` is null
and `active_coils_basis` explicitly says that seating and inactive end coils are not
established. Consumers must retain this unknown; do not subtract two coils or infer
rate, material, loading, or preload. Closed/ground or variable-pitch geometry which
cannot meet the complete tube/helix proof remains unrecognised.

A candidate must be one valid solid composed entirely of round tube supports and
their planar end caps. Coaxial cylinder patches merge before topology is examined.
All tube pieces must form one connected, unbranched open chain. Swept NURBS supports
and torus segments supply circular sections in either parameter direction: opposite
stations establish their centres, and 16 section samples check radius and planarity.
Sweep stations follow the spline knot count (at least 128, at most 8192 intervals).
Circular sections, centreline joints and tangent directions fit one axis, radius,
and linear axial advance against unwrapped angle, across the whole chain. At least
two turns are required; no sampled angular interval may exceed 30 degrees.

Wire radius, helix radius and axial fit residuals are bounded individually by
`max(0.001 mm, 0.001 * size)` (axial size is pitch), published in `tolerances`.
Cylinder axes must intersect within the wire-radius tolerance. Every finite segment
must cover its joints and follow the fitted helix chord; its caps may extend at most
one wire radius past the fitted circle intersections, both of which must meet the same
axial helix fit, including at the two terminal segments. Swept section normals must
follow the fitted tangent within 0.01 radians. A cylinder's slope must agree with
pitch with the chord correction to 0.001, and its midpoint must be perpendicular to
the radial vector within the helix-radius tolerance. These discrete chords are
validated as chords, rather than mistaken for exact helical arcs. A row or bolt
circle of pins, branches, a planar ring, local radius/pitch drift and thread or
knurl surfaces cannot pass the complete-chain proof.

Every face of the solid is claimed, including exposed segment end caps. This proved
whole-spring claim takes precedence over individual wire holes, bosses, fillets,
blends and surface fallbacks before accepted-member patterns are built. The record
and fingerprint retain those exact defining faces; `faces` also appears in the
record parameters. Draftwright can use `free_length` and `outer_diameter` for its
simplified view dimensions, and the remaining measurements for its spring data
table, retaining unknown active coils and omitting unstated rate.

## Partially obstructed through-hole exits

A bore spanning its local wall, with exterior planar rim support at both axial ends,
remains `bottom: "through"` when part of an exit disk opens into air and a planar
face covers the rest. Conflicting open/closed end supports alone are insufficient:
the recogniser checks the trimmed cap domains and verifies open and material
witnesses immediately beyond the mouth in the owning solid. Entirely capped exits
retain their existing flat/drill-point classification. Unsupported geometry remains
`unknown`.

Hole records (including members of hole patterns) gain optional `exit_obstructions`,
a nonempty list of `{location, faces, covered_fraction}`. `location` is the mouth's
axis point in file coordinates, independent of which end the hole uses as its
presentation gauge. `faces` contains ascending zero-based file face indices for the
covering caps. They and the exterior rim supports are consulted evidence, not new
defining claims. `covered_fraction` estimates the union area covered inside that
mouth's disk, divided by disk area, using 4096 equal-area polar samples and rounded
to four decimals. It is approximate, for warnings, not a machining clearance.
Both ends may have entries. Unobstructed and blind holes omit the field; older
records deserialize with no reported obstruction.

This additive optional field keeps recognition schema 6 and fingerprint format 3.
It is diagnostic context, excluded from hole specs and fingerprints; the corrected
`bottom` remains a fingerprint trait. Pattern members retain their own warnings.
Consumers should re-recognise affected files, use `through` for their callout, and
warn about the reported obstruction rather than infer an unobstructed exit from
`through`. The Python binding returns this same Rust JSON; there is no separate
Python hole-record schema.

## Local-run rectangular steps (#211)

The optional `local_steps` family publishes `LocalStep` records. It is a separate
family because draftwright addresses a feature by its rank within a family:
adding short-run records to `through_steps` would renumber saved
`f:through_step:N` Spec references. Full-length discovery, ordering, parameters
and defining faces in `through_steps` are unchanged. Recognition schema 6 and
fingerprint format 3 remain unchanged; the family is omitted when empty.

A local step is two complete perpendicular principal-plane wall regions meeting
along one continuous straight concave seam. Their run intervals must agree and
at least one end must lie inside the owning valid solid's run envelope. Both
transverse lips must be straight convex boundaries across the complete run;
at least one must reach the solid's transverse envelope, as for through steps.
Every wall region's external edges must cover exactly its rectangular perimeter,
allowing smooth coplanar subdivisions. Holes, oblique trims, interrupted seams,
curved or tapered walls and internally enclosed staircases are refused.

At each run station, the *entire* end edge of each wall must join convexly to
one connected coplanar region perpendicular to the run, within the same solid.
The two walls must identify the same region. A gap, different support per leg,
nonplanar end or competing support refuses recognition. These may be exterior
end faces or walls left by a secondary cut: an interior station does not imply
a concave closing wall. Concave caps belong to existing blind-recess families
and are excluded here. An exact inset volume probe must also prove the removed
quadrant empty; neither matching bounding boxes nor a short seam suffices.

Parameters are millimetres and file-coordinate vectors/points:

- `run_direction`: unit direction from `run_start` to `run_end`.
- `run_start`, `run_end`: the two 3D seam endpoints, published on the 0.001 mm grid.
  Their order follows the positive run axis in the recognition frame; it stays
  attached to the record under motion, including a reversal in file coordinates.
- `length`: run extent, measured before positional rounding, rounded to 0.001 mm.
- `height_direction`, `height`, `depth_direction`, `depth`: the unit directions
  from the concave corner to the two convex lips, and those distances rounded to
  0.001 mm. Height is the first leg of the canonical through-step section and
  depth the second (lexicographically lesser endpoint first in the recognition
  frame); these names do not imply world gravity. Directions make their meaning
  explicit after arbitrary rigid motion.
- `ends`: two `{scope, faces}` values in start/end order. `scope` is `solid` at
  the envelope and `local` at an interior station. `faces` lists ascending
  zero-based file face indices of the unique coplanar boundary region.

Only the two wall regions define the feature. End faces and the remainder of the
solid consulted by the volume proof are context, so an unrelated cut's end wall
is not claimed as part of the original step. All consulted geometry must have
strict valid-solid evidence. Fingerprints retain run length, the unordered pair
of transverse leg lengths, direction axes and endpoint scopes; positions and evidence face indices are not
shape traits. The canonical height/depth names may exchange under a principal-axis
permutation, so this presentation gauge cannot manufacture a resize.

For MFInstSeg, class 8 (`rectangular_through_step`) explicitly maps to both
`through_steps` and `local_steps`: a secondary cut may shorten a labelled step
without changing the two surviving defining walls. This is a mapping change,
not a selector inferred from labels. No other class correspondence changes.
The original box-minus-two-cuts controls, oblique-end and blind-end negatives,
full-length control, motion/publication tests and optional external repro are in
`tests/internal/local_steps.rs` and `tests/internal/mfinstseg.rs`; regenerate the
original controls with `tools/make_local_steps_fixture.py`.


## Polygonal oblique through steps (#212)

The optional `polygonal_oblique_through_steps` family publishes
`PolygonalObliqueThroughStep`. It is separate from `oblique_through_steps`
because draftwright numbers features by rank within each family (`f:<family>:N`).
Adding polygonal occurrences to the old array could renumber saved Spec references.
The old four-corner recogniser, parameters and ordering remain unchanged. The new
family is omitted when empty; recognition schema 6 and fingerprint format 3 remain.

The principal wall is a planar, simple, straight-edged polygon with **more than
four noncollinear corners**, without inner wires. Its boundary consists of one
complete straight concave seam and a chain of principal-axis edges or edges
owned by proved convex corner chamfers, all convexly joined to unique exterior supports. A curved support may trim a straight edge;
its normal does not measure the step. The perpendicular oblique wall is a
complete rectangle with that seam as one side. Both run ends must lie on opposite
sides of the same stock-envelope axis and have a unique common convex planar
terminal supporting the complete wall end. A seam between adjacent stock sides
is a triangular blind-step wedge and is refused, even after polygonal trimming.

The entire opposite oblique-wall edge must join one unique parallel, same-facing
convex lip. This lip may be inside the stock envelope after another cut, as in
MFInstSeg `_1002`; its position, rather than the stock box, measures the height.
Every principal-wall corner must lie on the oblique wall's outward side. The exact
principal polygon swept through the wall height must contain zero solid volume,
with only the existing tolerance inset along the sweep. Curved boundaries, holes,
tapered or interrupted oblique walls, ambiguous or concave bounding supports,
nonplanar far lips, unproved sweeps and local run ends are refused. This is deliberately conservative;
it does not claim all polygon-trimmed slanted steps.

`run` is the unit seam direction from the lexicographically smaller endpoint;
`length` is its full Euclidean length in mm. `at` is the seam midpoint.
`depth_direction` is the outward principal-wall normal and `depth` is the step
height in mm, measured to the opposite convex lip. `across_direction` points from
the seam into the principal wall. `floor_outline` retains every principal-floor polygon
corner as `[along_run, across]` from `at - run * length / 2`, canonicalised only by cyclic
start and walk reversal. It is never replaced by a quad or convex hull. Lengths
and outline coordinates publish three decimals; directions and `slant_degrees`
publish six. Unlike the legacy family's four-corner `wall_outline`, this field is
a variable-length principal-floor polygon, never the oblique rectangle. `through_axis` is an unoriented unit vector normal to
the two opposite stock-envelope run terminals, expressed in the record's
coordinate space. `slant_degrees` is `acos(abs(dot(run, through_axis)))` in degrees,
using unrounded directions. It is measured against that through axis, even when
another axis is closer to the seam: a seam 60 degrees off the through axis reports
60 degrees, not 30. Rigid mapping carries `through_axis` with the other directions
and retains the slant. The sign of `through_axis` has no geometric meaning.
`body_key` follows the existing oblique family. The two walls alone are defining faces; all boundary supports,
terminals and the far lip are consulted evidence. Rigid mapping carries the frame
and leaves the intrinsic polygon coordinates unchanged.

MFInstSeg class 10 (`slanted_through_step`) maps additively to this family and the
unchanged `oblique_through_steps`. Only defining walls count in scoring, never the
context supports. The mapping uses no labels to select records. Original generated
polyhedron controls cover a six-corner floor, a 60-degree x-through seam,
skew/taper refusals and an unchanged four-corner fixture; the external test checks `_1002` faces 20–21.

## Internal bevels (#213)

`internal_bevels` is a sibling family, with record type `InternalBevel`. It is deliberately
not merged into `chamfers`: draftwright persists `f:<family>:N` ranks, so appending a newly
recognised internal bevel to the chamfer sort could renumber saved Spec references. Existing
convex chamfer discovery, records, ordering and fingerprints are unchanged. The sibling is
omitted when empty and contributes no frame votes.

The initial contract is a **bounded rectangular planar strip** with one four-line loop,
two complete parallel spring edges of equal run span, and four planar neighbours joined
concavely. Exactly one pair must prove perpendicular transverse support planes within the feature
angular policy below. Both support
normals point into the cavity between the bevel normal's rays; neither may be within the
shallow-draft limit (normal dot product <= 0.05). Supporting wall areas must each strictly
exceed the strip area, and its larger leg cannot exceed 45% of the owning solid's maximum
extent. These are conservative treatment/role limits, not evidence of a machining history.
The other edges terminate the strip at both ends; taper, holes, curved terminals, split
boundaries and competing support pairs are refused. Every defining and context face must
belong to one strictly valid solid.

The internal family's perpendicularity and transverse-alignment tests use the shared
feature policy `AXIS_ZERO_COS` (0.01), not the kernel's 1e-9 smooth-arc threshold.
Contact, span equality and wedge insetting retain the bevel helpers' input modelling
precision with the kernel coordinate floor. Legacy chamfer/blend helper defaults are unchanged.

Intersect the actual supports at the strip midpoint to reconstruct the virtual `corner`.
`leg1` and `leg2` are Euclidean distances on those supports from corner to spring edge,
longer first; `leg1_direction` and `leg2_direction` are their outward unit directions.
`angle` is the acute angle between the bevel and leg1 on its actual support, not an
included wall angle. It equals atan2(leg2, leg1) on exactly perpendicular supports;
accepted STEP angular noise is measured in the real triangle rather than idealised away.
`run` is a unit vector, `length` its complete span, and `at` the strip midpoint at
the same station as corner. Positions and lengths publish millimetres to three decimals,
unit directions to six and angle to two. Sub-micrometre leg ties use descending support-wall
area only when neither the
published lengths nor the measured angle can change. This is intrinsic under rigid motion.
If support areas are indistinguishable, the two directions have no slot identity in the
fingerprint: Placement fields are represented by the run, defining-face centroid and graph,
not by named direction slots. Equal legs therefore remain an unordered pair for matching. Positions are carried
unrounded through rigid
publication before rounding once. Only the strip is defining; both supports and both ends
are context.

The open triangular wedge from corner to the spring edges, swept over the complete run,
must be wholly material by volume integration on the owning solid. Its boundaries are
inset by the input's modelling precision (at least the kernel coordinate floor); no missing
volume fraction is permitted and unresolved integration rejects the candidate. The bevel's
inward side must classify cleanly as material and its outward side cleanly as air;
ON and unresolved classifications are refused.

A sloped pocket floor normally has parallel opposing side supports, not a unique
perpendicular pair. A broad sloped wall fails support dominance or treatment size; draft
fails the support-angle/normal test. A slanted step has convex boundary joins and often a
triangular end, rather than four concave joins. A countersink is conical with circular rims,
not a four-line plane. An arbitrary ramp or web that cannot be distinguished geometrically
from an internal edge treatment is refused when these role proofs are ambiguous. Open-ended,
interrupted or nonrectangular internal treatments remain outside this initial contract.


## Pocket floor rounds (#228)

A closed rectangular pocket with an intact planar floor and analytic wall-to-floor
rounds is published as `pockets`, with `depth` measured from the mouth to the
planar floor (including the rounded portion). `corner_radius` still describes
vertical wall corners: concave cylinders whose axes follow the pocket depth.
Floor rounds instead are transverse cylinders under planar walls, or tori under
those vertical corner cylinders. Torus **minor** radius is the floor radius;
its major radius plus its minor radius must equal the supporting wall radius.

Two optional additive fields are omitted on existing sharp-floor records:

- `floor_radius`: positive millimetres, to three decimals, **only** when every
  floor boundary has a fillet and all raw analytic radii agree within the kernel
  coordinate floor. Omission means no uniform radius was proved, never zero.
- `floor_fillets`: entries `{face, wall_face, radius}` in ascending `face` order.
  Both indices are zero-based file face indices. `face` is the treated cylinder
  or torus, `wall_face` its tangent planar or cylindrical mouth wall, and `radius`
  its local floor radius in millimetres to three decimals. The treated faces are
  defining pocket faces and yield no unrelated `fillets` or `blends` records.
  Vertical corner cylinders remain cavity context, independently of these entries.

A partial floor has entries only for its treated boundaries; all other floor
boundaries in this admitted geometry meet their walls sharply. For the committed
`pocket_partial_floor_fillet.step`, the two long floor edges have radius 1.5,
its two short edges are sharp, depth is 4, and `floor_radius` is omitted. Mixed
radii likewise never produce a uniform radius merely because publication rounds
them to the same value. Mixed boundaries do not form pocket patterns until an
intrinsic edge-layout comparison is available; uniform floor radii join the
existing pocket pattern spec. Fingerprints include a proved uniform radius as a
size; face-index attachments are excluded from revision matching.

Recognition requires an inner mouth wire, four opposed inward planar walls,
optionally four proved equal vertical corner cylinders, and one entire closed
cavity on one valid solid. Every floor edge must meet either its sharp wall or a
single analytic tangent transition, every wall must reach the mouth and its
proved bottom role, and no unaccounted lower faces or external boundaries are
allowed. Shared/interrupted floors, islands, open run ends, square role ambiguity
and nonanalytic rounds do not acquire this proof. This is independent of the
legacy slot test, whose wall bottoms sit above the planar floor after filleting;
closed proofs are excluded from slot proposals. True through-slots and open-ended
recesses retain their existing geometry-based recognisers.

The three original build123d STEP controls are `pocket_plain.step`,
`pocket_floor_fillet.step` and `pocket_partial_floor_fillet.step`. Their generator
text and issue reference are in `tools/make_pocket_floor_fillets.py`; the first
two retain the issue-supplied bytes. Analytic tests check depth, both radius roles,
partial boundaries, face ownership and existing through-slot negatives.

Recognition schema stays `quiddity-rust/recognition/6`, following the existing
optional additive field convention (as for `exit_obstructions` and duplicate
references). Existing records omit both fields, deserialize with empty/unknown
floor treatment, and keep their previous fields and ranks. No fingerprint format
or crate version changes.

### FTC-08 inventory and section projection audit (#228)

The NIST FTC-08 closed cavity formerly published as `slots/0` on walls
`[213,214,215,216]` is now `pockets/8`: depth 1.27 mm, wall corner radius
3.18 mm (native 3.175), uniform floor radius 0.762 mm. Its attached floor
patches are cylinders `[82,84,85,135]` and tori `[61,62,63,64]`; floor 83
is claimed during floor completion. The face-specific inventory losses retain
historical slot/fillet baselines, and the physical, accepted and framed pocket
floors rise from eight to nine. No unrelated loss allowance is added.

`section_recesses` is a reconstructible **constant-section** contract, rather
than a second inventory of every pocket. FTC-08's floor 83 lies at z=46.99
and spans 11.176 × 157.226 mm; the mouth spans 12.7 × 158.75 mm at z=48.26.
The footprint grows through the 0.762 mm floor blend up to z=47.752, then
remains constant through the straight walls. Sweeping either the mouth or the
floor profile over the full depth would therefore invent void or material.
The four corner tori have major/minor radii 2.413/0.762 mm and nonzero Gaussian
curvature inside each patch. They cannot be the lateral surfaces of a linear
constant-section sweep, or any of the supported plane, cylinder or two-plane
envelope run ends. A truthful complete section record needs a richer contract;
`unsupported_support_geometry` is the correct current result. The detailed
pocket and its floor treatment remain available independently of that refusal.

There was **no former slot projection** to lose: `slots` is not among the legacy
source families projected by `section_recess_family::project`. Main's six
section records have defining faces `[156]`, `[222]`,
`[236,238,239,240,241,242,243,244,245,246,247,248]`,
`[249,250,251,253,254,255,256,257,258,259,260,261,262,263,264,265,266,267,268,269]`,
`[256,266]`, and `[254,268]`. Their geometry and evidence are unchanged against
origin/main, and none of their constituent faces belongs to this cavity.
The document's seventh section-family row is a newly explicit refusal because
`pockets` participates in projection; the number of actual section geometries
stays six. `ftc08_floor_fillet_pocket_replaces_only_its_slot_and_floor_rounds`
checks the native radii, changing footprint, torus curvature, complete pocket
ownership, exact surviving slot/fillet groups, and the six sections plus refusal.


### Floor-round review controls and refusal policy (#228)

A ball-end pocket with equal vertical-corner and floor radii uses spherical
corner patches in place of zero-major-radius tori. A sphere is admitted only
when its radius equals its supporting depth-axis corner cylinder's radius,
its centre lies on that axis at `floor + open_sign * radius`, and it joins the
two same-radius cylindrical floor strips smoothly. Its floor contact is a
proved floor vertex, rather than a shared floor edge. The per-face
`floor_fillets` list includes these point-contact corner patches; a fully
rounded r3/r3 floor reports uniform `floor_radius: 3`. Legacy vertical-wall
fillets retain their cylinders, while spherical bottom caps owned by the pocket
are removed from those fillets' defining claims.

A four-sided prismatic wall reading yields to the complete floor-treated pocket
only after pocket acceptance. The full pocket's additional floor-transition
claims do not prevent that containment proof. This removes both the duplicate
machining claim and its duplicate `SectionRecessRefusal`; the remaining single
section refusal is still correct for this non-constant-section cavity. The
same rule applies to the partial-floor fixture. Level/riser evidence can still
co-own faces under the existing evidence policy.

Legacy pocket seeds are retired on overlapping cavity membership, including
seeds that selected a different floor. Supported closed-pocket proposals leave
raw slot candidates available until reconciliation proves an accepted winner.
Thus a subsequently rejected pocket cannot erase its slot fallback. FTC-08's
physical slot count returns to its historical four; its accepted/framed counts
remain three, and only the obsolete physical-slot loss allowance is retired.

The r3.9-in-4-mm review part retains 0.1-mm straight walls but has four STEP
`REVOLUTION` corner patches stored as nonanalytic B-splines. They are outside
the native analytic radius proof. The complete closed floor is nevertheless
observed through smooth boundaries. No slot or shallow legacy pocket is issued
for that cavity. Optional additive family `pocket_refusals` publishes
`PocketRefusal {reason: "unsupported_floor_transition", faces: [...]}` through
the existing discovery-refusal contract. It claims no manufacturing faces and
adds no fingerprint; other supported cavities on the same part remain eligible.
Schema `/6` is unchanged, following the other additive discovery-refusal families.
The bounded-transition height used to validate this refused cavity is never
published as an inferred radius.

`tools/make_pocket_floor_fillets.py --near-misses-only` reproduces the independent
review constructions as small compressed STEP controls. The open blind slot,
island boss, floor through-hole, mouth-rim fillet and sloped floor do not receive
floor-fillet pocket records. Spherical, sharp-corner full-floor and deep-refusal
controls pin the positive and unsupported cases. The independent review source
files are read-only inputs; no fixture generator writes into that directory.

Feature matching retains its existing size-spec contract: adding a floor fillet
changes an existing pocket from an orphaned feature to a newly recognised feature
across revisions, just as a `None`→`Some` corner-radius proof does. This change
does not introduce a correspondence heuristic or alter other feature ranks.


## Circular blind steps with conical entries

A native flared quarter-cone can establish the opening of a circular blind step.
The original surfaces must prove a coaxial shared rim, a complete quarter-cylinder,
two perpendicular corner planes through its axis, and one perpendicular exterior
mouth plane, with convex entry joins on a common valid solid. Both conical rims
and both generators must be present. An interrupted cone or an entry tapering into
material does not establish an opening. The complete floor sweep must contain zero
material through the straight wall and the entry, up to the exterior mouth.

The `circular_blind_steps` record measures the **straight cylindrical portion**;
its length ends at the shared circular rim. The entry cone is consulted evidence,
rather than a defining face of that straight step. Its native shared radius is used
before presentation rounding, so a rounded cylinder-inventory diameter cannot
change seam incidence or round the newly admitted radius twice. No record fields
or schema versions change. Planar entries retain their legacy inventory-diameter
rounding: applying native publication there changes existing recorded radii.
This compatibility exception affects presentation, not seam incidence.
Analytic flared/inward-taper controls, independent volume
and placement checks, and a fractional-radius regression live in
`src/features/circular_blind_steps.rs`.

## Interrupted polygonal through-cuts

The optional `prismatic_through_cuts` family extends through-cut recognition to planar
convex polygonal sections whose walls are interrupted by other cuts. Its `PrismaticThroughCut` record contains
`cut` (a `SectionPassage` section and nominal prism), `extent: "observed_walls"`, and
`core_interval` (an axial interval with complete original-wall support). The nominal
`cut.run_interval` bounds the observed walls; its end planes are not asserted to be
physical mouths. The legacy `section_passages` end-plane contract is unchanged.

Discovery derives the run from adjacent wall-plane normals and accepts a closed
concave junction cycle, without assuming a principal axis or a wall count. A valid
owning solid, whole-face containment on the inferred prism sides, exact complete
side coverage throughout the core, and an empty prism through the owner's entire
axial envelope are required. Thus a surviving wall ring around a blind pocket does
not suffice. Only original supported wall faces are claimed; inferred mouth faces
are never claimed. Recovery is skipped on **any shared defining face** with an accepted cut record,
including partial overlaps. This avoids a second callout for a slot whose legacy
record owns only its two long walls. Context/evidence faces do not block recovery.
Precedence families are `holes`, `countersinks`, `double_d_bores`, `slots`,
`oriented_slots`, `rectangular_blind_slots`, `round_bottom_blind_slots`,
`semicircular_blind_slots`, `section_passages`, `section_recesses`, `pockets`,
`prismatic_pockets`, `edge_open_circular_pockets`, `edge_open_prismatic_recesses`,
`channels`, `v_channels`, `rectangular_through_notches`, `through_steps`,
`local_steps`, `angled_steps`, `paired_ramp_steps`, `oblique_through_steps`,
`polygonal_oblique_through_steps`, `circular_blind_steps`, `grooves`,
`annular_grooves`, `internal_bevels`, `chamfers`, `oriented_chamfers`, `fillets`,
`blends` and `interior_voids`. Body/additive families (including `turned_steps`,
`step_levels`, `risers`, `plates`, bosses and pads) do not own a machining cut.
Later residual passes run after this recovery and consume its accepted claims.
An optional recovery failure leaves those independent passes and floor completion
available, rather than blocking them on missing planar-cut claims. Open/branched wall chains and
profiles needing missing walls or curved sides are not inferred.

This is a post-recognition extension, represented in recognition/6, fingerprints,
and face accounting; the evidence document's legacy physical-candidate registry
does not acquire a new candidate family.

## Hole mouth edge breaks (#244)

`HoleRecord` has an optional, default-empty `mouth_chamfers` array. Each entry has
`axis` (into the hole), `location` (wide-rim centre in file coordinates), `diameter`
(the cylindrical mouth diameter), radial `size`, axial `depth`, `angle` (degrees
from the transverse top face, **not** included cone angle), and `faces` (cone face
indices). Both exterior ends may carry entries; array order does not assert a
machining entry end. Empty arrays are omitted, and old hole records deserialize
with no treatments. Rust struct literals must supply the new field.

This is a composed hole detail, like a mouth attachment, rather than a separate
correspondence feature. `internal_bevels` proves concave planar strips, whereas
this is a convex axisymmetric edge break. Existing turned `chamfers` describes
external-cylinder edges. Making every small hole-mouth trim another independent
chamfer feature also adds indistinguishable features on symmetric hole patterns,
expanding existing correspondence ambiguities. The nested record describes the
machining note without introducing another feature identity. Hole patterns retain
per-member treatments, but exclude them from the matching specification: a missing,
interrupted or slightly noisy cone must not split a bolt circle or change its ID.

The cone and its support plane are consulted evidence on the hole and must share
its valid solid. The cone is claimed through the nested treatment; the plane is
only support. `Record.faces` remains the core defining face set used by the
fingerprint and stable anchor. `Record.claimed_faces()` additionally iterates the
proved `mouth_chamfers[*].faces` for holes, and complete face accounting uses that
union. It does **not** claim arbitrary consulted faces, support planes, or nested
exit-obstruction faces. Patterns retain their existing defining member-face union;
the individual hole owns its treatment. Existing readers can keep using the core
`faces` for saved IDs and inspect the sub-record for chamfer selection.

Treatment annotations do not enter hole fingerprints: the bore's identity and
all existing correspondence evidence stay unchanged. Consumers updating chamfer
notes should compare the treatment parameters. The `face_claims`/`as_compare`
benchmark scores defining-face unions; it does not score these nested treatment
claims. Its unchanged scores therefore do not imply unchanged complete face
accounting. No class mapping, score floor, or correspondence allowance is changed.

Recognition requires a full circular cone, a coaxial internal cylinder sharing
its minor end circle, and an outward-facing transverse plane sharing its major
circle. Both joins must be convex. The cone must flare outward at an extreme of
the **whole** cylinder stack, excluding a counterbore floor/step edge even if that
edge has a small cone. A flare strictly below the countersink family's 1.5
major/minor-radius boundary and an axial leg below half the mouth radius prove
a small edge break. No boss, absolute size, file axis, or dataset identity is
required. Large seats retain `csink`/`countersinks` recognition. Split or interrupted
cones without a complete single-face circumference are not claimed by this rule.
The unrounded angle must be within 15°–75°, inclusive, to exclude shallow tapers
and nearly transverse lead-ins. This symmetric band is the same from the axis or
the top plane; the recorded angle remains measured from the top plane. It is a
conservative chamfer-family policy, not a claim that other cone angles cannot be
manufactured. Split-cone support needs an angular-coverage and shared-seam proof,
not just adding patch angles (which could overlap); the two-half-cone regression
pins the current abstention while preserving the cylindrical hole.

At the shared 1.5 major/minor-radius boundary, a 0.5 × 45° edge on a ⌀2 bore
remains a ⌀3 × 90° countersink. A small screw seat and an edge break can have
identical geometry, including the same adjoining cylinder and relative depth.
Those dimensions cannot establish machining intent. Keep the existing scale-free
family boundary rather than infer a chamfer from bore depth or change saved seats.

Depth semantics stay cylindrical, consistent with existing plain-hole/countersink
handling: neither `HoleRecord.depth` nor `cbore.depth`, `spotface.depth`, or
`outer_steps[*].depth` includes an entry cone. `location` remains the cylindrical
opening/gauge; the treatment carries its own top-face location. In #244, `cbore`
remains `{diameter:5, depth:2}`, and the entry has
`{diameter:5, size:0.25, depth:0.25, angle:45}`. A drawing referenced to the boss top
should read **⌴⌀5 ↧2.25**, plus **0.25 × 45° at counterbore mouth**: add the entry's
axial depth to the 2.0 mm cylinder span. Printing ↧2.0 from that top face would
locate the floor incorrectly. This is callout guidance; the existing dimension
planner is not changed to generate chamfer notes.

### Saved face anchors

On #244, `holes/0` retains faces `[3,10]` and anchor **3**. Its sub-record claims
cone face `[1]`, so the cone is explained without shifting the hole's anchor to 1.
No standalone feature is added, and existing hole, pattern and chamfer IDs and
fingerprints stay unchanged. Consumers can select the nested cone faces for a
chamfer note while preserving the cylinder as the saved feature reference.
