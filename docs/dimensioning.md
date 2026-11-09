# Shared dimension requirements (slice 1, #141)

`quiddity::dimensioning` decides which manufacturing dimensions a prismatic part
requires. It has no sheet, view, label, units formatting, lane, placement, or editor
scheme. Recognition and the existing CLI are unchanged. Specify and Draftwright
integration belong to their own repositories.

```rust,no_run
use quiddity::{dimensioning::{Input, Policy, plan}, read_step_file};
let part = read_step_file(std::path::Path::new("plate.step"))?;
let input = Input::from_recognition(&part, vec![], Policy::default())?;
let (required, conveyed) = plan(&input);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`cargo run --release --example dimension_plan -- plate.step` prints the typed input,
required set and conveyance records as JSON. This is an example, not a new CLI path.

## Input and coordinates

`Input::from_recognition(&Part, Vec<Datum>, Policy) -> Result<Input, String>` infers
the part frame, names its directions by their nearest file axes (positive towards
those axes when this remains a proper rotation), transforms the part in memory,
and runs recognition. This geometric convention chooses the height direction
that Draftwright's front projection uses without introducing views. A strongly
rolled frame whose nearest signed axes would reflect it retains the recogniser's
proper representative. Face indices remain those of the caller. Coordinates are
in **that part frame**, in mm; `input.frame.to_world(point)` recovers file coordinates.
Frame refusal is an error. Recognition and its normal frame API remain unchanged.

The adapter supplies `holes`, `patterns`, all principal `planes` (including later-slice
planes for datum lookup), `steps`, `cartesian_envelope`, `deferred`, `round_axes`,
`bounds`, `datums`, `specified` and `policy`. `StepLevels` contains the ladder axis,
its accepted height supports and tied shoulder planes. Merely appearing in `planes`
does **not** make a plane a requirement. `Deferred` contains a face-derived id,
faces and the owning family. A deferral counts an ownership claim, not a dimension:
several recognised families can describe the same face. A turned/round or whole-stock
envelope has three explicit deferred Cartesian extent claims. A corroborated cylinder
whose two radial diameters exactly fill the box establishes a round envelope even
without a turned-step record (for example a threaded axial body). Later slices will
supply the owner's diameter/length/across-flats grammar.

The ladder axis comes from the **file orientation**: the inferred frame's directions
are named by their nearest file axes, and its Z direction is the height ladder.
Rotating the file can therefore choose a different ladder direction; this is the
same geometric convention as Draftwright's default front projection, not a layout
choice or a claim of rotational invariance for the requirement grammar.

The height ladder uses only interior horizontal support proposals (0.5 mm clustering,
1% projected area, interior margin min(0.6 mm, a quarter of the body's height)).
Pocket/recess floors and walls, slot walls, boss/pad tops, turned faces, chamfers,
fillets and other later-slice feature faces remain owned by their accepted families;
they never become raw plane intervals. Pocket floor and cylindrical boss cap ownership includes consulted
constituent faces from the same recognition run, rather than just defining walls. Hole floors and bounded axial annular shoulders adjacent to recognised hole walls
belong to their hole size/depth; larger entry/support faces remain step evidence.
Unclaimed interior planes outside the ladder/shoulders are explicitly deferred as
`non-step plane`. Outer prismatic extents remain required; a curved prismatic side
can use a synthetic support. A turned envelope acquires no Cartesian requirements.

Consumers with existing recognition can build `Input` directly, using the same
contract: unique face-derived feature ids, sorted face sets, finite coordinates,
positive sizes, valid axes `0..3`, normalized drilling directions and canonical
size-role names. When known, `Plane.body` and `Support.body` must agree. Do not substitute
recognition ordinals for identity. `Hole.depth` is blind depth; `traversal` is a
through segment's material length, used only to count drilling across narrow slits.
`Hole.bottom` reuses recognition’s typed `Bottom` enum.
`specified` contains the file's already stated requirements, with its tolerance
classes and ids; the adapter does not read or reinterpret semantic PMI itself.

## Output and PMI

`plan(&Input) -> (Vec<Required>, Vec<Conveyed>)` returns sorted, deterministic values.
`Required` has an id, `Kind::Size { feature, role, kind }` or
`Kind::Location { from, to, axis }`, nominal, count, optional `PatternRole`, tolerance,
optional reference carrying ids, and all covered feature ids. Separate bore, blind
depth, counterbore, spotface, outer-step and countersink quantities are separate size
requirements. A drawing may combine these into one callout; a callout count and an
atomic requirement count are therefore different measurements.

`Support` retains feature id, face set, body, point and whether it is a physical
plane rather than a point/axis. Requirement ids derive from faces and semantic
roles; group ids derive from sorted member ids. Actual patterns use `pattern[...]`;
counted unpatterned sizes use `multiple[...]` (MultipleElements). Pitch endpoints
are actual member-hole supports, with that member's stable id and faces. On a grid,
the lexical first member at each station witnesses its pitch; other rows are
covered by the pattern membership and interval count. First-member line locations
also name the actual first hole. They are stable under recognition
list permutations, not across arbitrary renumbering of the source B-rep. Use
quiddity correspondence to transport ids across changed topology. Characteristic
numbers can attach to these ids. `Conveyed` explicitly maps retired member or
coincident requirements to surviving ids with a reason. Its `by` is flattened;
consumers need not follow chains of omitted ids. Explicit deferrals are also returned
as `Conveyed` with an empty `by` and `why = "deferred: owned by <family> (later slice)"`;
these records promise no surviving requirement. An empty `by` always means a
deferral; a zero-length station walk cannot convey a requirement, and a member
location without a nonempty pattern proof stays required. Their typed metadata is in
`input.deferred`. `covers` retains displaced claims.

`Required::dimension_kind(resolve)` returns haecceity's `pmi::DimensionKind` using a
consumer-supplied feature-arena resolver. Planner string ids never pretend to be
PMI arena indices. For coincident claims, the resolver can allocate a per-requirement
group from `covers`; the helper maps the kind but does not populate or validate the
consumer’s feature arena. `Pattern::pmi_feature(resolve)` returns
`Feature::Group { kind: PatternOfFeatures, members }`. For counted unpatterned
sizes, consumers can build a `MultipleElements` group from `covers`. Support face
sets can be lowered to `Feature::Items` with `Anchor::Face`. Angles lower as angular
sizes with Equal selection; the nominal remains in degrees.

## Rules and provenance

Adapted from pzfreo/draftwright `origin/main` **1c830977** (same copyright owner):

- `dims/holes.rs`: patterns before single holes; full evenly spaced principal-axis
  lattices use their centre and one pitch per varying axis. Lines use their first
  member and counted intervals. Irregular/rotated lattices fall back to individual
  locations. Complete identical machining specs share counted sizes. Signed drilling
  directions, blind depths and every enlargement remain spec constraints.
- `bridge/holes.rs::established_with`: at least two identical members; reject missing,
  repeated, oblique or coaxial members. Five holes establish a bolt circle; smaller
  circles need a parallel hole/boss axis within 0.5 mm of the centre. Circle diameter
  and centre state the pattern. Through traversal lengths do not split identical
  requirements. Plain coaxial through segments separated by less than one bore
  diameter count one drilling; wide gaps, blind or enlarged bores remain separate.
- `dims/pmi_policy.rs::{origins,selected_origins}`: Automatic uses the first usable
  datum ordered by label then face set per axis; otherwise the box minimum. Policy
  Minimum/Maximum/Datum overrides it. A missing or unusable datum falls back to
  minimum. The same origin selector applies to holes, step heights and shoulders
  on every axis. Maximum origins traverse a counted height ladder downwards;
  interior origins use individual baseline heights. Zero distance creates no dimension.
- `dims/levels.rs::{ownership,project,repeat}` and `dims/steps.rs` ownership boundary:
  one height ladder, base to each unowned level, plus riser shoulders located from
  the part box. Vertical risers must have their foot on a retained body level; ramps
  need both ends on retained levels or the envelope. Equal shoulder stations merge
  to the thousandth. Three or more uniform rises (within 10% of their mean) share
  one counted rise, including the rise to the top when it matches. Multi-axis plate
  base tops belong to plate thicknesses. Through-step ownership is resolved to a
  fixed point against levels, shoulders, plates and extents: sections that need
  their own leg grammar own their corner levels and box-ended shoulder sites and
  are explicitly deferred. A recognised turned profile has no prismatic ladder.
  Plate thickness and independent through-step section requirements are later-slice
  feature owners, not anonymous baseline intervals.
- `dims/holes.rs::{merge,consolidate}` and `dims/redundancy.rs`: coincident raw spans
  merge only with equal tolerance contracts, count and pitch intervals, within
  0.0001 mm, independently of views or labels. General, Basic, Untoleranced and
  exact Explicit contracts are distinct between generated claims. A specified
  requirement replaces the generated equivalent for the same feature and physical
  span **regardless of tolerance**, with an explicit conveyance reason; a counted
  size must still cover the same members and count. Equivalent locations can have
  reversed endpoints, and a plain authored location can replace a generated
  first-member location. Remaining coincident claims prefer specified then lexical
  id; all covered claims survive. Pitch and non-pitch never merge. Different bodies
  never merge coincident supports.
- `lint/redundancy.rs::path`: a bounded breadth-first station walk detects closed
  physical-plane chains per axis, body and tolerance contract. Point locations and
  pitches alone never supply plane edges; a survivor carrying a merged plane claim
  retains that interval’s graph ownership. Already specified requirements are authoritative;
  generated determined intervals are conveyed by the existing path. With
  `policy.references`, untoleranced determined intervals remain as references.
  Toleranced requirements never become references. Counted step repeats retain
  their overall height: Draftwright lint's `level_span` recognizes only
  `step_height.length.h...`, deliberately excluding the aggregate
  `step_height.length` repeat from its station graph. We follow that boundary and
  do not expand an approximate mean rise into exact virtual plane stations. Unrelated equal values do not
  determine one another. Layout and optional sheet references play no role.

Defaults request general tolerance, automatic origins and no references. There is
no precision/label key: any stated precision that must prevent merging belongs in
`Tolerance::Explicit`. Inputs and recognition are never mutated.

## Tests and comparison

The module tests re-express Draftwright requirement assertions from `dims/holes.rs`
(raw ends, tolerance separation, specified priority, id tie-break), `bridge/holes.rs`
(principal axes, identical/blind/through specs, pattern acceptance and small-circle
corroboration, slit drilling counts), `dimension_rules` (grid, blind/through,
counterbore/spotface/countersink quantities, determinism), `repeated_dimensions`
(shared parallel-row pitch, distinct pitch stations, location/level ownership),
`ownership_rules` (one statement per span, toleranced dimensions never reference),
`coverage_holes` (each accepted shoulder station), and `h5_chains`/`lint/redundancy` (physical
station paths, closed chains and optional references). Additional tests exercise
PMI lowering, body isolation, the real recognition adapter, pocket-floor and turned-envelope deferral, counted step ladders and conveyed-id validity.

Skipped deliberately: holes' oblique-view ordering/rim tips and slot/boss precedence;
bridge JSON field parsing (typed input), boss-frame parsing and rendering checks;
all three `dims/recesses.rs` tests (pocket maximum depth, edge-open pocket location,
pocket-on-box location); lint's turned-chamfer/whole-boss plane test; h5 turned
ladders, boss/slot ownership and editor omissions; repeated slot widths/boss heights;
coverage's double-D, independent seat, profile-angle and ink assertions; all view,
placement, text, precision, rendering and editor-policy assertions in the named suites.
Slots, recesses, bosses, turned ladders and dimension schemes remain later slices.

`tools/compare_dimensioning.py` runs the example and `draftwright plan` sequentially
on a plate and all entries in Draftwright's `tools/quality_parts.txt`. It emits a
JSON-lines report outside the repository: total required quantities, Draftwright
hole/level/envelope statements and expanded quantities, plus **every** unmatched
quantity and its source id. It includes unplaced wants, restores wants conveyed
by hole tables from their `covered` records, and expands compound callouts. Axis matching maps the inferred frame back onto file axes; a reversed
origin can still produce complementary offsets. Use the original plate (its SHA is
in the issue's recorded binding), not Specify's annotated output, which selects
Draftwright's authored-PMI policy. Build the read-only Draftwright source into a scratch target directory and record its revision/version with comparison results. Reports are not snapshot goldens.
