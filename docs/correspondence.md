# Correspondence between revisions

**Status:** design accepted with draftwright-rust's ADR R3 (2026-10-07); implemented in
`src/correspondence/` (fingerprint version `quiddity-rust/fingerprint/3`). See
[Implementation](#implementation) for what was built, the thresholds and how they were tuned, and
what is not done.

## The problem

A STEP file is revised. A hole moves, a boss grows, a slot is added, or the whole part is
re-modelled in another position. The decisions made against the old revision (a drawing's
callouts, a part's fits and datums) should carry over to the new one. There is no construction
history to follow, only two files.

Face indices and quiddity's occurrence refs are stable within one file only. So recognition
results gain revision-stable fingerprints, and a pure function, `correspond`, matches two
results.

## Fingerprints

Every recognised feature and every face gets a fingerprint in the result. It is deterministic,
serialised, and versioned (`fingerprint_version`).

- **Feature:** three parts.
  - *Semantics:* family and subtype, its intrinsic sizes (diameter, depth, width, length,
    radius, angle, as the family has them), and its pattern count and pitch.
  - *Placement:* its axis or normal, and its position in the part's frame.
  - *Neighbourhood:* its subgraph of the attributed adjacency graph (Joshi & Chang 1988). This
    covers the feature's own faces, and the faces it meets with how it meets them: surface type,
    whether each shared edge is convex, concave or tangent, and the meeting face's role (a
    planar face it enters, the floor it ends on, a boss it sits on). It is recorded as a
    canonical, order-independent signature.
- **Face:** surface type and parameters (radius, half-angle, axis), its centroid, area and
  outward normal, and its adjacency signature (the types of its neighbours, and whether each
  edge is convex or concave).

A consumer stores the fingerprint beside each decision, so the old STEP file isn't needed to
rebind.

## `correspond(old, new) → Correspondence`

### 1. Align

- **Find a rigid motion.** From features and planar faces whose fingerprints are distinctive in
  both revisions (unique holes, unique large faces), solve for a rigid motion: Kabsch over
  candidate pairs, with RANSAC.
- **Accept it** when enough fingerprints agree under it.
- **Otherwise** use the identity and mark the result `unaligned`, which down-weights positions in
  the costs below.

### 2. Match the features

- **Costs:** for each family, a cost matrix over old × new. Each cost sums three weighted
  terms:
  - *semantics:* subtype, intrinsic sizes, pattern count and pitch;
  - *neighbourhood:* the distance between the two AAG signatures;
  - *placement:* axis, and position (once aligned).

  The terms do different jobs:
  - **Neighbourhood** is unaffected by rigid motion, and holds when a feature moves or resizes.
    It carries the match where position is weak, in an unaligned revision or for a moved
    feature. It also tells apart features that are alike in size and place but sit in different
    surroundings, such as a hole through a flange and a blind hole into a boss.
  - **Placement** separates what the neighbourhood can't: truly symmetric repeats, such as the
    instances of a hole pattern, whose neighbourhoods are identical.
  - When placement is down-weighted (an unaligned result) and such repeats remain within the
    margin, they are reported as ambiguous, never guessed.

  The weights, like the thresholds, are tuned on revision pairs.
- **Assignment:** one to one, by the Hungarian method, with an explicit "unmatched" column.
- **Classification:**
  - **carried:** low cost, and the winner clearly ahead of the runner-up;
  - **adapted:** the same feature with changed parameters, reported field by field;
  - **ambiguous:** the margin is small, typically pattern instances and symmetric features. The
    candidates are reported; none is guessed;
  - **orphaned:** an old feature with no match;
  - **new:** a new feature with no match.

### 3. Match the faces

Faces matter for datums and GD&T targets.

- **Seed** from the faces owned by matched features, and from faces that coincide or overlap
  after alignment (the same surface, overlapping by at least 80%).
- **Propagate** by adjacency signature over the attributed adjacency graph, preferring a face of
  the same surface type when breaking ties.
- **Classify** with the same five classes as features.

### Output

The Correspondence holds:

- the alignment, and whether it was found;
- per old feature and face: its class, its match or candidates, and what changed;
- the new features and faces;
- a list of questions for the ambiguous entries.

It is exposed in the library, in the CLI (`quiddity correspond old.json new.json`) and in the
JSON-lines and MCP surface beside recognition.

## Congruence: `congruent(a, b) → Option<(Motion, face pairs)>`

`correspond` finds the likeliest motion between two revisions and tolerates differences.
`correspondence::congruent(&Part, &Part)` answers a stricter question, for identifying identical
parts (a pack's or a parts list's quantities): is there a proper rigid motion taking *a* exactly
onto *b*? It returns the motion and each face of *a* paired with the face of *b* it lands on, or
`None`. It reads geometry only, never product names or other metadata.
`congruent_with(&a, &af, &b, &bf)` is the same with the parts' `Fingerprints` already made
(`congruent` makes them with no features), so a caller comparing each product against several
groups fingerprints each part once.

- **Candidates**, in order: an exact B-rep match (`Part::same_brep` in haecceity: the same
  faces, edges and solids in order, with bit-equal defining geometry and trims) or the identity;
  the translation taking *a*'s area-weighted centroid onto *b*'s; then `align` on the two parts'
  face fingerprints.
- **Proof.** A candidate other than the exact match is accepted only when every face pairs one
  to one with the same surface type, adjacency signature, parameters and area (within 1e-7
  relative), centroid (within 1e-5 mm) and mean normal under the motion, and when samples of
  each trimmed face (0.1 mm deflection) lie within 1e-5 mm of its partner, in both
  directions.
- **Why the translation comes before `align`.** On a rotationally symmetric part, `align` can
  return a symmetry of the fingerprints that is not a symmetry of the B-rep: on a hex bolt, a
  ±120° turn of the hexagon that moves the seams. The proof rejects it, so two translated
  copies would be missed without the translation.
- **Reflections are never considered.** A mirror image is congruent only if it is also a
  rotated copy (the part is achiral). The face proof rejects a mirror image on its own; a
  candidate that is not a proper rotation is refused before it as defence in depth.
- **Only exact copies of some parts.** A part with a face whose area the kernel cannot
  integrate, or with an unresolved face or edge, matches only through `same_brep`.

## Background

- **History-based persistent naming needs the construction history.** Examples are Capoyleas,
  Chen & Hoffmann (CAD 1996), Kripac (CAD 1997), Marcheix & Pierra's survey (SMA 2002), OCCT's
  TNaming and FreeCAD 1.0's element maps.
- **Onshape's re-import matcher is history-free.** It is described by Baran & Schulz,
  US 11,288,411: exact seeds, then overlap, then adjacency-signature propagation, with the rest
  left unmatched.
- **Jones et al., "B-rep Matching for Collaborating Across CAD Systems"** (SIGGRAPH 2023) learn
  the scoring on top of that matcher. They report a residue that stays genuinely ambiguous.
- **Bidarra et al. (2005)** argue for anchoring on features rather than boundary faces.
- **Joshi & Chang (CAD 1988)** introduced the attributed adjacency graph used here for both
  features and faces.
- **Vandenbrande et al. (US 8,576,224)** align two B-reps by matching planar faces with the
  Hungarian method.

## Tests

- **Invariance:** the corpus's invariance variants (moved, rotated, mirrored) must correspond with
  everything carried.
- **Revision pairs:** a capture tool builds revision pairs in build123d (move a hole, resize a
  boss, add or remove a feature, pattern changes, a whole-part motion). Each pair has its
  expected classes, and differences get verdicts as in the other families.
- **Thresholds:** the cost cut-off, the margin and the inlier fraction are tuned on those pairs.
  No published values transfer.

## Implementation

`src/correspondence/` (2026-10-07). `quiddity part.step` now writes each family's records as
before plus a top-level `fingerprints` (and, since then, the recognition `document`,
`src/recognition.rs`); `quiddity correspond old new` takes two such results (or
two STEP files) and writes the `Correspondence`. In the library: `correspondence::recognise`,
`fingerprint`, `correspond`, `correspond_with` (explicit `Thresholds`), `congruent` and
`congruent_with` (`congruent.rs`).

`correspondence::recognise(&Part) -> Recognition` is the supported recognition entry point.
It infers the part's frame, transforms the loaded geometry in memory, recognises there and
maps records back. It does not re-read STEP. `frame` reports `Inferred` or `Refused { reason }`;
directions without corresponding file axes use unit vectors and explicit record origins.
The top-level frame is provenance only; see [schema 6 migration](recognition-6.md).
Frame selection (for #81) compares complete orthogonal sets of three direction lines. Each
distinct analytic surface gives one vote to a set when its plane normal or cylinder axis
lies within `cos(angle) >= 1 - 1e-10` of one of its lines (about 0.0008°). Coplanar planes
and coaxial cylinders of equal radius count once, including disconnected patches: plane
normal offsets, cylinder axis distances and radii match within the kernel's 1e-6 mm
coordinate floor. Splitting a supporting surface into more B-rep faces cannot add votes.
Candidates come from perpendicular material direction pairs, with the existing perpendicular
tolerance of 0.001 in cosine. The pair's original direction classes vote even when the second
axis is orthogonalised; the third line is their normalised cross product. The highest distinct
surface count wins; the file's axes win an equal count when two material directions establish
them. The winner's leading line is kept, but its roll about that line (#91) goes to a less-voted
candidate set containing the line when the box around the material in that set (every edge's
sampled points) is more than 1e-4 smaller in volume: three parallel angled slots no longer turn
their rectangular plate (14 votes against 6, box 0.53 times the slots' box). An equal vote keeps
the tie rule above. The box is intrinsic, so this choice follows a rigid motion. This preference retains the original material basis within the tight direction band;
recognition does not snap its real tiny rotations to exact coordinate axes. Publication alone
may snap a near-axis motion when no part point moves beyond the coordinate floor (#70).
Within a winning set, area within the legacy cosine-0.999 angular band ranks
its axes, followed by the exact class signatures; face offsets orient them. Area is summed
independently about each line, so nearby directions remain distinct and face order does not
choose their representative. This retains the aligned rc/rd axis order. Counts keep a large minority angled surface from choosing the
frame's roll; tight direction matching keeps real 1° features distinct, independent of STEP
face order. A rigid motion carries the intrinsic frame unless the moved file axes win this
vote. Parts with only one established material line retain the axial gauge, including its
unobservable roll; this rule does not invent a second material direction.

The CLI, service and recognition document use this same route. `recognise_placed` is removed.
Caller-space recognition, discovery, inventory, context construction and evidence builders
are crate-private by default. Record types remain public at their existing paths. The legacy
`frames::prepare_framed*`, `working_part`, `FramedPart` and `FramedError` helpers and the
`RecordFrame::CallerSpace` variant are private to crate tests. Caller-space recognition,
discovery and inventory are implementation details; tests exercise them internally. Rust
snapshots own the regression record; see [development](development.md#regression-snapshots) for regeneration and review.


### Fingerprints (`fingerprint.rs`, version `quiddity-rust/fingerprint/3`)

- **Features** are read from each family's serialised records through an explicit table,
  `FAMILIES` in `fingerprint.rs`, keyed by the family's field in `Features`. It lists every field
  path of the family's records (`cbore.diameter` into a nested record, `section.segments[].radius`
  into each item of a list) with its role, and a field takes the role of the longest listed path
  leading to it:
  - *size*: an intrinsic size by its path; several numbers (a gusset's two legs, a ramp's
    half-widths, a pocket's arc radii) are ordered by the frame or the record's canonical order,
    so they are kept smallest first as `<path>.<i>`;
  - *trait* (the subtype): a string, boolean or null; a nested record's presence (a counterbore);
    a list of strings, sorted and joined. Pattern families of untagged variants add their shape
    (the record's field names);
  - *count*: a list of member records, by its length (a pattern's `count`, a thin wall's `pairs`,
    a void's `openings`);
  - *axis* and *placement*: the first axis field present (a vector or an axis letter) is the
    feature's axis; positions, frame-aligned bounds, spans, directions, axis letters and signs
    move with the part and are otherwise not compared. Placement is the axis and the area-weighted
    centroid of the defining faces; derived families (hole and gusset rib patterns, the table's
    `members`) take their members' faces;
  - *derived*: read only through sizes the family derives from them, unchanged by motion: a
    plate's `thickness` (hi − lo), a turned step's `length` (hi − lo), a gusset rib's `thickness`
    (its bounds), a through step's two section legs, and an edge-open pocket's or recess's
    `run_length` (its run interval), `opening_width`, wall or straight-segment lengths and arc
    sweeps (absolute, since a sweep's sign follows the section's handedness in the frame);
  - *ignored*: face and body indices, sampling diagnostics (a void's grid pitch and sample
    counts), a fit's residual, a thin wall's heuristic history hint.

  `fingerprint` panics on a family missing from the table, or one with neither defining faces
  (`Features::defining`) nor members: a wiring defect. `tests/internal/wiring.rs` recognises every STEP
  fixture and corpus part and fails on a family not wired that way (defining-face lists one per
  record), on a record field with no role, on a table path no record has, and on a feature whose
  fingerprint has no size.
  - *neighbourhood*: in draftwright's bridge format, with the arc labels below (which depart
    from the bridge's on nearly tangent edges).
- **Faces:** the effective surface type (a B-spline exactly a plane, cylinder… counts as one),
  its intrinsic parameters, axis or outward normal and support point, area, centroid and mean
  outward normal (`Part::face_moments`, the boundary quadrature of `face_mass`), neighbours with
  their arcs, and the sorted adjacency signature. Two departures from the kernel, both for
  invariance: a pair of faces whose shared edges all have normals within 1e-3 (1 − cos, about
  2.6°) of parallel is recorded `smooth` (all within 1e-3 of opposite, `unknown`), a departure
  from the kernel's labels and so from the bridge's (a genuine crease shallower than 2.6° reads
  as smooth), because the kernel's convex/concave call there follows round-off
  and changed with a mere translation on nist_ftc_07, mfcadpp 11512 and sm-hanger; and a face
  whose quadrature gives no area (nist_ftc_10 face 174, stream F's defect) has an unknown area
  and its edge samples' mean as centroid.
- `scale` is the square root of the part's area, which motion leaves unchanged.
- **Versions.** 2 (2026-10-08, review finding H5) replaced version 1's global list of 32 size
  names, under which any other numeric field, a field nested two deep and the items of a list
  were dropped: every plate, edge-open recess and interior void had no sizes, so a resize of one
  was carried without a trace. Version 2 adds those sizes (and the gusset rib, turned step and
  through step ones above), drops version 1's traits from incidental fields (a chamfer's corner, a
  thin wall's history hint, a groove's profile key) and counts a thin wall's face pairs rather
  than its unpaired faces.

Version 3 retains the complete size catalogue and changes recognition measurements:

- Hole and recess dimensions are measured before position publication. Decimal half-way
  measurements within the kernel coordinate floor use ties-to-even consistently.
- Equal pocket wall separations retain all walls and use original face identity to select
  an interpretation. Equal distinct hole mouths use the original cylinder support direction.
- Groove supports are joined to each floor end symmetrically, including overlapping bands.
  An independently proved helical root remains in its thread record.
- Void samples use a box-centred lattice with stable cell counts at integral pitch ratios.
  Unproved chamber candidates retain their complete data in diagnostic document records;
  refusals make no recognised feature claim and have no feature fingerprint.
- Plate face bands use their extreme-plane midpoint and retain unrounded coordinates,
  so thickness is a difference of intrinsic measurements rather than published grid positions.

Version 2 files must be recognised again before correspondence. The version check refuses
mixing either revision with an older fingerprint. The Rust snapshot owns the regression record; independent geometry and motion references
remain mandatory. Python-written fingerprints remain readable test inputs.

### Correspond

- **Input**: fingerprints read from a file are checked before use, and refused with an error
  rather than indexed out of range: the version, a finite positive scale, each face at its own
  index, and every face a feature or a face's neighbours name among the faces (review finding L2).
- **Align** (`align.rs`): candidate pairs are features and faces whose motion-free fingerprints
  agree, with at most four candidates either way; hypotheses come from every pair and triple of
  pairs (or 3000 of each, sampled deterministically), solved by Horn's quaternion method (Kabsch
  without reflections); the most inliers win, then the smallest residual, then the motion nearest
  the identity. When every agreeing anchor lies on one line (a turned part), the alignment is
  *axisymmetric*: positions are compared by their distance along and from that line and
  directions by their angle to it. The turn about the line is then fixed from the off-axis
  repeats (features and faces alike with up to 64 candidates): each pair votes for the turn
  taking the old item onto the new one, and the turn most old items agree with wins. A part with
  k-fold symmetry about its axis has k such turns (`fold`); the one giving the motion nearest the
  identity is taken. Only a part with no off-axis repeats stays axisymmetric. `symmetric` records
  that another motion fits as well.
- **Features** (`mod.rs`, `assign.rs`): per family, cost = semantics (trait mismatch and mean
  relative size difference) + neighbourhood (multiset distance) + placement (half axis, half
  position). Aligned, a displacement costs its logarithm in tolerances (so millimetres separate
  repeats); unaligned, it is linear and weighted 0.1. The Hungarian method runs on the (old + new)
  square with an unmatched option at half the cut-off. A match is ambiguous when forcing another
  candidate (or none) costs less than the margin more; the duals' reduced costs bound that, and
  only the candidates under the bound are solved again. Matches are carried when nothing changed
  within the tolerances, adapted otherwise with the changes field by field. Unaligned, there is
  no common frame, so placement is left out of the changes: carried then means unchanged in
  everything but placement (the thickened plate's top face, 5 mm higher, is carried).
- **Faces** (`faces.rs`): seeds are faces on one surface after alignment that overlap (estimated:
  areas within 80% and centroids within 20% of the smaller face's size; the fingerprints carry no
  boundary to intersect), then the faces of carried and adapted features assigned within each
  pair. Propagation pairs an unmatched face next to a matched one with an unmatched face next to
  its partner across an edge of the same kind, costed by the face cost plus the share of its
  matched neighbours whose partners the candidate does not touch; a pair is taken when each is
  the other's best by the margin. What remains is assigned per surface type under the feature
  rules (up to 400 faces a type), then propagated again.

### Thresholds and how they were tuned

`Thresholds` (`src/correspondence/mod.rs`) documents each value. In short:

| Threshold | Value | Basis |
|---|---|---|
| `same_rel` | 1e-6 | round-off: moved analytic fingerprints agree to ~1e-9 (except the kernel's sphere-pole areas, below) |
| `freeform_rel` | 1e-3 | B-spline quadrature moves with placement by up to ~2e-4 |
| `position_tol`, `angle_tol` | 1e-5 × scale, 1e-5 rad | round-off, far below a design change |
| `anchor_candidates` | 4 | keeps two- and four-fold symmetric faces as anchors |
| `turn_candidates` | 64 | a turned part's off-axis repeats (24 alike holes on the spool's two flanges) vote for the turn |
| `min_inliers`, `min_inlier_fraction` | 3, 0.5 | aligned revision pairs keep ≥ 70% |
| `placement_reach`, `unaligned_placement` | 0.25 × scale, 0.1 | a whole-reach move costs 1 |
| `cutoff` | 1.0 | matched revision costs are 0.06–0.5 |
| `margin` | 0.05 | unaligned swaps: 20 mm apart 0.064 (kept), 5–10 mm 0.016–0.032 (ambiguous) |

### Validation

`tests/internal/invariance.rs` reads every corpus part under translation and principal rotations,
checks the same feature faces, then checks correspondence on the same readings. It also
checks recognition under a generic rotation in the part's inferred frame. The ledgers pin
all remaining differences; a stale entry or an unlisted difference fails. Analytic tests
cover tolerance boundaries, measured half-way sizes, reflected sample lattices, and plate
thickness under a translation smaller than its former position-publication grid.

`tests/internal/correspondence.rs` checks the frozen revision pairs: dimensions, added and removed
features, changed patterns, rigid whole-part placements, and unchanged source face claims.

The ten revision pairs exercise these results: moving a hole adapts its position; resizing
a boss adapts its diameter; adding a hole makes it new; removing a hole or boss orphans it;
moving the whole part carries every feature and face; moving the part and one hole adapts
that hole; growing a four-hole row to six adapts the pattern count and adds two holes;
resizing a fillet adapts it; and thickening the plate leaves the pair unaligned, adapting
distinct holes and the 20 mm row by depth while the 5 mm row is ambiguous. The test checks
the unchanged features as carried and the carried features' faces (patterns aside) against
their partners. Its exact expectations and listed verdicts remain in the fixtures.

### Limits

The remaining section run-interval publication cases are explicitly pinned in
`references/correspondence.json`: absolute endpoints still use the public three-decimal grid.
Fixing them requires an intrinsic run-length representation or a versioned frame origin
along the run. The recognised geometry and its current sizes remain visible in the output.

Mirrored revisions are not aligned: the motion model uses proper rotations. Families
explicitly read along world Z change when the caller rotates Z away from the part; default
recognition uses the part's inferred frame. Symmetric geometry can still leave correspondence
between equivalent repeated copies undetermined.

Face overlap is estimated from fingerprint areas and centroids, rather than intersecting
the faces' boundaries. The cut-off has not been stressed by a competing removed-and-added
pair in one family; such a pair may be classified as an adapted feature instead of an
orphaned feature and a new one.

The fingerprint content changes across this stack before the final version bump. Merge
the seven layers together so version 2 fingerprints with intermediate semantics are never
released independently.

For #134, boss placement is the complete measured axial midpoint, and plate placement
restores the projected strips removed from its skins by proved convex edge treatments.
These two families' neighbourhoods contract those treatments to their supports.
These intentional value corrections stay in fingerprint format `/3` (as for #71/#92);
face fingerprints still describe the physically trimmed geometry. An aligned matched
plate seeds mutually unique skin planes without requiring their areas to remain alike.
