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

The optional `annular_grooves` family contains `AnnularGroove` records and is omitted when
empty. This additive family keeps `quiddity-rust/recognition/6`; existing families retain
all their records. It describes a blind annular face recess of rectangular radial section,
such as a face-seal housing. It makes no nominal O-ring size or intended-function claim.

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
claim by another individual feature family.

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
