# Face accounting (recognition document 6)

Every newly emitted document, including `quiddity serve`'s `result.document`, uses
`quiddity-rust/recognition/6`. `face_accounting` is an array of exactly `face_count`
objects: array index is the original file face index. Accounting itself does not change record order, IDs, parameters, face claims,
fingerprints or evidence status. Precedence is:

1. `{"group":"feature","record_ids":[...]}`: every published non-refusal record
   naming the face, in document order, including derived and body-description records.
2. `{"group":"refused","refusals":[{"record_id":"...","reason":"..."}]}`:
   every named refusal covering the face, in document order, retaining its reason.
3. `{"group":"body","reason":"..."}`: one of the positive evidence codes below.
4. `{"group":"unexplained"}`: no sufficient explanation. Review these faces.

Thus a refusal cannot erase a feature claim, and body evidence cannot erase either.
A feature face may belong to several records but belongs to only one accounting group.
Refusals overlapping a feature remain available in `records`.
Sealed cylindrical cavities belong to `interior_voids`, including their closing
faces. Their void-shell cylinders are excluded during hole recognition, so neither
`holes` nor `hole_patterns` duplicate those face claims. This exclusion does not
remove the interior-void claim or reclassify its faces as body geometry.
The shared closed-shell proof also supplies void records below the air grid's
resolution, so suppressing a sealed bore cannot leave its skin unclaimed.

After reconciliation, flat-bottomed blind `holes`, `pockets` and `prismatic_pockets`
also claim an already-consulted planar floor perpendicular to their inward depth
direction, at the deepest end of their walls. Every floor boundary edge must have
exactly one other incident face belonging to that feature's walls. Floors continuing
into a slot, step or another cavity remain context. Existing manufacturing claims take
precedence; body-level `step_levels` and `risers` evidence is retained alongside the
new recess claim.
identical wall sets in multiple recess descriptions share the new floor. Otherwise,
competing new candidates are assigned in order to holes, pockets, then prismatic
pockets. This adds faces without changing recognition decisions or other families'
claims. Section-recess claims are unchanged pending #206. Discovery and inventory
evidence still describe the original walls and consulted floor; these final claims
are completed in the same post-reconciliation pass as drill-point tips.

Readers accept older documents: absent `face_accounting` deserializes to `None`,
which means **unknown accounting**, never “no unexplained faces”. New empty parts
have `Some([])`. `SCHEMA_VERSION` and `schema_version(features)` now always select 6;
`LEGACY_SCHEMA_VERSION` remains the historical version-3 constant for readers.

## Positive body rules

### `stock_box_side` (unchanged)

The face must be an outward planar side of its own solid's bounding box **in the
inferred part frame**, with a closed four-line loop visiting the four distinct box-side
corners in order. Every edge must join adjacent corners (no diagonals). The entire
rectangle spans both transverse solid extents. Interior loops (holes/pockets opening
through the stock surface) are allowed; they do not make the surrounding stock a feature.
Notched, rounded, split or partial outer loops do not qualify for this code.

The solid must have full geometric validity. All its faces must be native planes,
cylinders, cones, spheres or tori, and boundary curves must be lines, circles or ellipses, or native-surface
NURBS trims whose finite positive weights and complete finite control-point hull
certify containment in the solid envelope (within the distance tolerance). The
convex-hull property supplies this certificate; sampled spline extrema alone
never do. A hull leaving the envelope vetoes evidence for the whole solid. All three box
extents must be finite and exceed 100 times the distance tolerance. The face must
belong to that solid. Plane normal must point toward the corresponding extreme;
axis alignment tolerance is `1e-9`, and corner/extreme distance tolerance is
`1e-7 * max(1 mm, solid maximum extent)`.

### `stock_box_notched` and `stock_box_split`

Both retain the validity, native geometry, finite extents, outward principal normal,
solid membership and tolerances above. The **entire face** must lie on its outward
box support plane. In addition, one of its own boundary vertices must be a corner
of that box side (both transverse coordinates at solid extrema), or a recognised
fillet/chamfer/blend face sharing an edge with it must replace such a corner.
For a replaced corner, the treatment must be a convex principal-run cylinder or
a planar chamfer, with two perpendicular outward planar supports at box extrema.
The treatment must reach a box extreme along its third (run) axis. Chamfer edges
to both supports must be convex; cylinders must point outward. An unrelated or
unrecognised treatment, a concave blend, or a treatment away from the box corner
cannot supply a corner. Another face's surviving corner cannot establish evidence
for an isolated island. The outline may contain
any number of line/conic edges and may have cut openings.

A potential underlying platform vetoes the new rules. Collect all interior planar
faces with the same outward normal; sort their axial levels and group consecutive
levels within the distance tolerance. For each group, merge the face bounds. If
that union reaches either solid extreme in **each** transverse axis, abstain.
For a replaced corner, only lower faces connected to the candidate through a
shared bridging face spanning both axial levels enter this veto. A remote inward
wall on the opposite block is not an underlying platform. For surviving corners
the existing, broader platform veto remains unchanged. This intentionally rejects
some stock surfaces beside large cut floors. It also
rejects an unrecognised edge pad's cap: the lower platform reaches the remaining
perimeter. Combining split lower platforms catches a rib that divides its base.
Grouping uses geometric distances, never coordinate rounding or hash bins.

`stock_box_notched` means this is the only outward face on that support plane.
`stock_box_split` means several outward planar faces share it, as when a through
cut splits a stock side. Each accepted fragment supplies its own surviving or replaced stock corner.
The unchanged rectangle rule is tried first, so existing evidence keeps its code.

The positive argument is a surviving vertex of the solid's rectangular envelope,
on its outward support plane, rather than an interior cut surface. Pocket floors,
cut walls and step treads inside the envelope cannot qualify. Isolated boss caps
lack that corner; edge pads and ribs on a lower exterior platform are vetoed even
with no feature records. Tests ask the geometry routine directly, independently
of recognition precedence, for a round boss, an edge pad and a rib. Extremality
alone was evaluated and rejected because it accepts the round boss cap. No label,
feature count or dataset identity selects body evidence. Recognised corner treatments
and plates supply only the geometric premises specified here.

This is conservative evidence for box stock, not a reconstruction of design intent.
Unusual profiles still need review. There is no turned-body rule: maximum radius
can also describe a flange or an unrecognised boss, and the stronger full-axial-span
cylinder candidate supplied no unclaimed corpus examples with two complete end
rings. Cones can themselves be drawn tapers/chamfers. Curved faces remain unexplained
unless a record or refusal accounts for them. Invalid, shell-only,
freeform-bearing, degenerate, or frame-refused input supplies no body evidence.

The rules are symmetric under frame axis permutation and reversal. It is evaluated
on the already normalized part, before records map back into file coordinates;
face indices survive that mapping. A caller-space parity result supplies no body
evidence. Motion tests cover Z30, X30 and composed Z17+X23, with translation.

### `plate_edge`

After box evidence, a recognised plate supplies its own defining face groups and
raw `lo…hi`. An eligible wall is a native principal plane perpendicular to the
plate axis, in the same valid solid, sharing an outer-loop edge with a plate face
at **both** ends of the slab. Both joins must be convex, and any concave boundary join vetoes the wall (a
recess floor or riser can otherwise resemble a slab edge). Its full axial extent
must equal the slab. Inner loops (holes, slots, pockets) cannot supply this evidence.

Among perimeter walls meeting both slab ends, including taller walls, its plane
must support the perimeter in its outward normal direction: no parallel perimeter
wall may lie further outward. Opposite-normal walls also constrain this support.
This rejects inward walls and returns into rectangular through-notches. Oblique
walls conservatively abstain, including V-notch flanks. The perimeter is local to
the plate's own face groups, rather than the complete part envelope, so a plate
between wider attached blocks can still explain its exposed straight edges.
The slab-local test is supplemented by the same solid's other principal walls:
none with a positive overlap along this wall's run may lie farther outward,
even outside the slab. A thin strip beside an unrecognised step otherwise has a
locally supporting edge which is actually a recessed shoulder. Only overlap of
positive length counts; attached blocks beyond the ends of a waist do not veto
its perimeter.

### `stock_wall`

A native principal planar wall must extend outside a recognised plate's slab;
a notch or waist cut through a plate alone cannot borrow that plate's stock
evidence. If its **entire native plane** is on the solid's outward envelope
boundary, that supporting plane supplies independent stock-skin evidence. This
route still requires two distinct convex straight-edge contacts: one original
box-evidence face excluding plate defining faces, and another original stock or
plate face. Extrema touched only at a vertex or an inward plane do not qualify.

An **interior** wall needs stronger proof: it must be perpendicular to the plate
axis. A parallel cut end face cannot borrow an unrelated plate's evidence.
Two original box-evidence faces, excluding
plate defining faces, must meet the wall across convex straight boundary edges.
One support is perpendicular to the plate axis, with its edge covering the wall's
entire run; the other is perpendicular to the run, with its edge covering the
wall's entire axial height. Thus two opposite stock end caps, partial contacts,
and a stock face plus a plate face cannot certify an arbitrary feature wall.
Any concave boundary join vetoes the wall: stock anchors alone cannot erase a
cut floor/riser. Both supporting faces must be direct neighbours; neither blend hops nor point
contacts qualify. Only the original box rules seed this pass: new plate edges and
stock walls never seed more stock walls. This deliberately requires an associated
plate, and abstains on stock-wall candidates without one.

Feature and refusal precedence remains unchanged. These rules can supply geometric
evidence on a face already named by a record, but accounting retains the record's
claim. In particular they cannot turn step-level or polygonal-boss faces into body.

## Record face coverage (for #100)

After recognition, reconciliation and section-recess projection have completed,
a `drill_point` hole includes its consulted cone faces when their apex is on the
hole axis beyond its deepest defining ring and they share that ring with a defining
face. Mouth countersinks are not tip faces. This retains the original recognition
and valid-solid evidence, without promoting unrelated cones.

A rectangular slot (`corner_radius` present, `end_radius` absent) includes both
planar end walls only when there is one unique inward wall at each raw axial end,
spanning the full depth and the width between its corner tangencies. Each end
must join both defining long walls through inward cylindrical corners of the
proved radius, with centres at the footprint tangencies. Width and corner-radius
checks use their existing two-decimal publication grid while geometric fits use
raw walls/cylinders, not rounded sizes. A missing, ambiguous, inward-reversed or
unconnected end abstains for both ends; the solid must be geometrically valid.
The original feature selection is unchanged.

A polygonal boss includes its consulted free-end cap when that plane faces along the
boss axis at the boss's `base` or `top` and lies within the side ring's transverse
extent ([recognition-6](recognition-6.md#polygonal-boss-caps)). On CTC-01 this is the
hexagon top, face 104. V-notch flanks through a plate edge are V channels with a
rounded trough, and rectangular through-notch walls are `rectangular_through_notches`
records, so neither needs body evidence; plate-edge and stock-wall rules still abstain
on them.

Schema stays `quiddity-rust/recognition/6`; fingerprints stay
`quiddity-rust/fingerprint/3`. Expanded face claims intentionally change affected
feature fingerprint `faces`, area-weighted `position` and `neighbourhood` within
version 3, including a derived pattern where a changed member contributes faces.
Sizes, traits and the part's face fingerprints are unchanged. The CTC-01 regression
compares complete origin/main fingerprints and requires correspondence to pair
both changed holes and the rectangular slot.

## Independent label measurement

MFInstSeg's stock class is **24**, not an absent instance or an unmapped recognition
family. The pinned AAGNet [class table](https://github.com/whjdark/AAGNet/blob/e0e36b7a12a7f01a29d7be36efc22730d293a1bd/dataset/Utils/parameters.py#L26-L50)
places `stock` at index 24; its [generator](https://github.com/whjdark/AAGNet/blob/e0e36b7a12a7f01a29d7be36efc22730d293a1bd/dataset/feature_creation.py#L167-L170)
initially labels the box faces with that index. Classes 0–23 are labelled features,
including classes without a corresponding recognition family.

The fixed, hash-verified 200-model ratchet retains its existing caller-space feature
precision/recall calculations. Separately it measures the supported default framed
document. Each model and the total record `feature_as_body` (labels 0–23 assigned
body; must be zero), `stock_unexplained` (label 24 assigned unexplained; may not
increase), `stock_body`, counts by body reason, and the four group counts. Read/recognition failures count all labelled
faces as unexplained and remain hard failures in the test and baseline generator.
Labels never enter the recognition or body algorithm.

The baseline format is version 2 for the added accounting fields. Regeneration with
`tools/update_mfinstseg_baseline.py` checks unchanged identities, input hashes,
mapping and supports, non-regressing feature scores, zero hidden feature faces and
non-increasing stock-unexplained counts per model. Changes require baseline review;
raising a limit is never automatic. Dataset files remain external and read-only.


### Disjoint safety sample

For expansion of body evidence, select 2,200 models **before** looking at their
results. Sort all 62,495 STEP stems lexically, remove the fixed 200 baseline IDs,
and from the remaining 62,295 choose `remaining[i * 62295 // 2200]` for
`i = 0..2199`. No failed, invalid, frame-refused or inconvenient model is replaced.
The SHA-256 of the selected IDs, each followed by a newline, is
`0cc1624fd59d444600d738a65445f845ec000893578aee73a2c273285922c155`.
The same `mfinstseg_score` example measures every model; it exits unsuccessfully
if any labelled feature is body. Check recognition/label failures and model counts
in its output too. Keep this measurement external rather than adding 2,200-model
latency to the fixed ratchet. The recognition schema remains 6: these are additional
body reason codes in the existing accounting field, with no record/schema shape change.
