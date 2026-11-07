# Changelog

All notable changes to quiddity-rust and haecceity. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Alpha 1's gates and their evidence are
in [docs/alpha1.md](docs/alpha1.md).

## Unreleased

### Added

- Optional additive `superseded_by` references on groove-derived holes and bosses,
  and `source` references on section recesses with identical source-family defining
  faces (#204, #206). All records, claims, order and IDs are retained. The MFInstSeg
  scorer honours `superseded_by` while retaining `source`-marked recesses for each
  class's existing mapping; schema 6 and existing consumers remain compatible.

- `quiddity completions SHELL` prints a shell completion script (bash, zsh, fish,
  powershell or elvish) to stdout; the README gives the install one-liners. `quiddity
  --version` prints the version.

### Changed

- Accepted flat-bottomed blind holes and blind pockets now claim a planar floor when
  every boundary edge joins their own deepest walls. Shared floors remain context;
  existing claims and recognition decisions are retained (#205, #207).

- The `quiddity` command line (the binary and the Python console script) is parsed by clap
  instead of by hand. For everything that worked before, commands, flags, defaults, exit
  codes, output and refusals are unchanged, as are the messages for values checked by quiddity
  (`--part`, `--mode`, `--presentation`). Help and usage errors are clap's (`Usage: quiddity
  …`, on stderr with exit 2 as before). Newly refused or changed: `pmi json` usage errors (a
  missing argument or `-o`, an unknown command, an unknown, repeated or valueless option) exit
  2 instead of 1, with clap's message (a bad `--part` value is still refused with exit 1);
  `evidence --framed` with no file exits 2 (was 1, `--framed` read as the file); a trailing
  flag with no value is reported by clap; `-h`/`--help` after a command (`parts`, `evidence`,
  `fingerprints`, `capabilities`, …) shows its help and exits 0 (was an error: 1 where read as
  a file, 2 for `capabilities`); `-V` prints the version (was read as a file, exit 1); a file
  to recognise named `completions` must be given as `./completions`. Newly accepted: a positional after
  the flags (`evidence --framed part.step`, `pmi read --part 1 part.step`), `-- FILE`, and `--option=VALUE`
  (`--output=out.step`). The command line is the root crate's default `cli` feature, so a
  library build can leave clap out (`default-features = false`).

## 0.4.8 (2026-11-06)

Upgrading from 0.4.7: recognition schema `/6`, fingerprints `/3` and the `haecceity.pmi/1` schema are unchanged. The PMI reader string moves from `haecceity-pmi-read/17` to `/19`.

### Added

- `StepFile::placed(&self, keep: &[usize])` reads selected placed body instances in file
  coordinates, keeping their original STEP entity and instance identities for PMI
  attachment (#194, for #111; thanks to @xu9299).
- General notes now have a default AP242 Notes saved view, facing their annotation plane
  and framing the part and notes together upright. Add preserves foreign views/defaults
  and reuses its own Notes view. Edition-1 Add writes skip the optional view; Replace
  upgrades edition 1 to edition 4 only when the view can be written. In assemblies,
  only the first eligible part gets a Notes view. Notes on several planes or with
  several text orientations produce a factual finding instead. `PmiRead.saved_views` lists saved views,
  and replace/remove clean up the writer's Notes view. The CLI reader binding is now
  `haecceity-pmi-read/19`: default views no longer produce a face-less shape-aspect finding,
  and material identifier fallback preserves absence of a separate name.

  Notes view windows lie on the view plane in camera coordinates, with explicit 3D
  placements, and the Notes model lists its planes' callouts directly, matching NIST CTC-02
  and SFA 5.41 (#198).
- **Rust API:** `PmiRead` (not `#[non_exhaustive]`) gains `saved_views: Vec<SavedView>`, so
  struct literals and exhaustive patterns need it. New: `pmi::SavedView`. `WriteReport`
  (non-exhaustive) gains `saved_view_findings`.
- **CLI and Python:** write reports gain `saved_view_findings` as an optional key.

### Changed

- Materials without a separate name repeat their ID as description, avoiding empty
  semantic text while preserving the reader round trip (#198).
- PMI writes give new draughting models unique names to satisfy UR1, including on
  assemblies without notes. Appended models use names such as `PMI callouts (2)` or
  `datum feature symbols (2)`; preserved presentation linking models also use suffixes (#198).
- Contributions are taken under Apache-2.0 with a Developer Certificate of Origin sign-off
  (`git commit -s`) instead of the Contributor License Agreement; `CLA.md` and its workflow are
  removed, and a DCO check runs on pull requests from forks (#192, #197).
- **PMI presentation follows the CAx-IF PMI Recommended Practice 4.1 (#193):** source leaders are
  tessellated within their callouts (no bare polyline curve occurrences); placeholders carry a
  direct axis placement, the standard PMI type names and links to their semantic PMI; curve sets
  use Table 18 names (`note`, `label`, ...). Alignment moves from placeholder names to AP242
  identification metadata, and older files remain readable. PMI writes add the recommended-practice
  reference to FILE_DESCRIPTION, replacing an older PMI reference and keeping everything else.

## 0.4.7 (2026-11-04)

Upgrading from 0.4.6: recognition schema `/6`, fingerprints `/3` and the `haecceity.pmi/1` schema are unchanged. The PMI reader string moves from `haecceity-pmi-read/15` to `/17`.
- **Sealed interior voids are not holes (#189):** a closed internal cavity (a void shell of the solid) is reported only as `interior_voids`, no longer also as `holes` and `hole_patterns`. Every sealed shell, spheres included, now gets an exact `closed_shell` record from its shell volume, so existing void records' volume and sample fields, and their fingerprints, change. Existing `interior_voids` ids keep their order. Holes that open to the outside are unchanged.
- **PMI round trip on the NIST CTC/FTC models (#188):** all ten available CTC/FTC AP242 files round-trip with empty differences, and the NIST strict test runs in CI.
  - Dimensions with no stated value are written through unchanged, with no refusal from `write` or `validate` (Python no longer raises `PmiError` for them), no invented value and no nonconformance finding.
  - A projected zone whose end has no geometry is kept and written; the reader reports it as a `fact` finding (it was `nonconformance`).
  - Anonymous attribute units are matched by content, so PMI carried into a new file (`quiddity brep --pmi`) binds the same unit; that path also writes the standard mass units it needs.
  - Replace keeps datum support shared with PMI it can't read, and rebinds it only to equal replacement datums (`WriteReport.retained_shared`).
  - Upgrade repair R3: when the written PMI needs a newer AP242 edition, INTEGER representation items written as exactly integral REALs are written as INTEGERs, each reported.
- **Rust API:** `FindingKind` gains `Fact` and `UpgradeRepairKind` gains `R3IntegerLiteral`; neither enum is `#[non_exhaustive]`, so exhaustive matches need an arm. `pmi::write::WriteReport` (non-exhaustive) gains `retained_shared: Vec<u64>`. New: `Finding::preserved_fact` and `Part::shell_mass` (signed volume and area of a closed shell).
- **CLI and Python:** write reports gain `retained_shared` as an optional key; PMI JSON findings can now have `kind: "fact"`.

### Details
- Recognize sealed cylindrical voids only as `interior_voids`; exclude their
  cylinders from holes, hole patterns, and hole-derived records while retaining
  complete void face accounting. Exterior-connected holes are unchanged.
  Every sealed shell, including spheres and cavities missed by the air-sampling
  grid, now receives a `closed_shell` record using shared shell evidence and
  boundary-integral volume. Existing sealed void records' volume/sample fields
  and fingerprints change accordingly; sampled record IDs retain their order.

- Finish #142's strict CTC/FTC STEP → PartPmi → JSON → PartPmi → STEP gate;
  run the NIST-only test in Linux CI and retain the opt-in corpus-wide run.
- Preserve dimensions with no stated value without a refusal, invented measure
  or nonconformance, including their principles and qualifiers. Preserve projected
  zones whose end has no geometry and report that statement as `FindingKind::Fact`.
- Reader binding advances to `haecceity-pmi-read/17` for the changed findings and
  anonymous unit identities; the JSON schema is unchanged.
- Resolve anonymous attribute units by portable content, reusing or writing equal units in the destination. Replace retains
  auxiliary support shared with unread PMI, rebinding only equal replacement
  datums, and reports it as `WriteReport.retained_shared`.

- Upgrade repair R3: when the written PMI requires a newer AP242 edition, INTEGER
  representation items written as exactly integral REALs are written as INTEGERs, each
  reported; `UpgradeRepairKind` gains `R3IntegerLiteral`. NIST CTC-04 needs it.

## 0.4.6 (2026-11-03)

Upgrading from 0.4.5: recognition schema `/6`, fingerprints `/3`, the `haecceity.pmi/1` schema and the PMI reader string `haecceity-pmi-read/15` are unchanged.
- **Add mode keeps the file's own bytes (#185):** every Add write now keeps pre-existing carriers, usages, controls and presentation at their original ids. This reverses 0.4.5's Add-mode normalisation, which broke `datum_feature.WR2` when adding a datum to NIST CTC-01. Any Add write that would change an original entity fails atomically with `WriteError::AddPreservation`, which names the entities. That covers R1/R2 upgrade repairs and AP214 keyword rewrites; use Replace for those. Skipped normalisations are reported in `WriteReport.preserved_normalizations` as `legacy-text-rechunking`, `kept-usage-factoring`, `functional-sar-isolation` or `legacy-endpoint-retirement`.
- **Source callout positions (#183):** PMI writing accepts an optional `haecceity.presentation/1` companion: Rust/JSON `write_with_presentation`, Python `presentation=`, and CLI `--source-presentation` on `quiddity pmi write` (exactly one selected part) and `quiddity pmi json import`. The CLI flag exists only in json-import builds. `/1` adds the optional `Graphic.leader`; readers that validate against the 0.4.5 schema (`additionalProperties: false`) reject documents that use it. Without a companion, #183 changes nothing.
- **Notes layout (#184):** unplaced feature notes become their own leadered callouts, and the general-notes block moves clear of drawn ink. Written presentation changes for files with feature notes or a crowded notes position; four writer goldens changed (`pmi_gaps`, `pmi_gaps_gram`, `spool_fits_intent`, `assembly_plate_pin_intent`).
- **Reports:** `WriteReport` adds `presentation_warnings` (placed text edge-on or overlapping, and feature notes whose glyphs can't be drawn) and `preserved_normalizations`. The CLI `pmi write` report and the Python report gain both as optional keys; `pmi json import` adds `presentation_warnings`. Python `writer_findings` gains both categories.
- **Rust API:** `WriteError` gains `AddPreservation` and is not `#[non_exhaustive]`, so exhaustive matches need an arm. `json::presentation::Graphic` gains the pub field `leader` and is not `#[non_exhaustive]`, so struct literals must supply it.

### Added

- Notes blocks use actual projected strokes and row text cells, selecting the first clear downward offset instead of avoiding whole callout boxes. Feature-note extent matches frame placement.

- Generate individual leadered callouts for unplaced feature notes; keep the general-notes block clear of generated, source-placed, and kept file ink.

- Source callouts honour /1 text alignment, attach leaders to the box edge towards
  the tip, and refuse conflicting placements on a shared datum symbol. JSON CLI
  import accepts `--source-presentation` with xcaf requirement ids.

- Source placement requires the `json-import` and `presentation` features.
  The CLI source flag is hidden without `json-import`; builds without presentation
  drawing refuse source input clearly. Minimal builds retain their previous features.

- PMI writing accepts optional source callout planes, text positions and leader
  tips through Rust/JSON `write_with_presentation`, Python `presentation=`, and
  CLI `--source-presentation FILE`. Placed notes have their own note-linked
  callouts. Invalid placements fail atomically; edge-on text and overlaps warn
  without moving supplied positions. Saved views remain a later phase.
- `haecceity.presentation/1` adds optional `Graphic.leader` (a model-space point
  in millimetres). Old companions still read. `WriteReport` adds
  `presentation_warnings`; #183 changes no writer golden.

### Fixed

- Add upgrade refusals name the required repairs and entity ids, explain that
  Replace retains a part with no PMI, and warn that it re-authors existing PMI.
  Retained legacy aliases no longer generate classification certificates or gain
  checker exemptions; before/after checks retain their original findings. Reuse
  across representations sharing a context is limited to Add preservation.
  The CLI and Python reports carry `preserved_normalizations` records (`{id, normalization}`).
- **Add preserves original entity bytes:** every Add write keeps pre-existing
  carriers, usages, controls and presentation at their original ids. Add no longer
  retires legacy endpoint wrappers, isolates functional SARs or rechunks retained
  legacy text, superseding 0.4.5's normalization behavior. Skipped normalizations
  appear in `WriteReport.preserved_normalizations`; schema upgrades requiring entity
  repairs now fail atomically in Add (use Replace for those repairs).
- Adding a datum on a kept face or pattern emits a separate `.T.` `datum_feature`,
  fixing NIST CTC-01's `.F.` perpendicularity carrier (`datum_feature.WR2`). Only
  faces with kept usages get independent GISUs in new representations; uncovered
  faces use ordinary carriers. Pattern datums retain their member scope. Non-datum
  additions also get independent usages when safe reuse is impossible. Each narrow
  CAx-IF §5.1 exception names the actual aspect and preserved carrier in
  `WriteReport.geometry_carrier_exemptions`; unrelated source aspects are not exempt.
  Kept carriers are never promoted in any mode; datum leaves created in the same
  write remain reusable. No EXPRESS validation rule is exempted.

## 0.4.5 (2026-11-02)

Upgrading from 0.4.4: recognition schema `/6`, fingerprints `/3` and the `haecceity.pmi/1` schema are unchanged. The PMI reader string advances to `haecceity-pmi-read/15`. Semantic PMI reads as before, with two exceptions: the datum findings below, and aspects that 0.4.5 writes named `haecceity.feature/1`. Old files are unaffected by the second.
- **Writer (#181):** written STEP links PMI to geometry the way CAx-IF PMI v4.1 §5.1 and §6.5 recommend. Each face has one shape aspect, which every dimension and tolerance on it reuses, and datum features are referenced directly. Semantic values are unchanged, and OpenCascade reads every checked-in written golden as before. A narrow exemption covers two shared-carrier shapes that OpenCascade 7.9.3 and NIST SFA 5.41 misread in the strict form. They are reported in `WriteReport.geometry_carrier_exemptions`, which the CLI and Python report also show as an optional `geometry_carrier_exemptions` key. The thumbwheel golden also keeps one allow-listed presentation-only carrier.
- **Reader (#180):** a datum whose feature reaches no face is now an `Unresolved` finding. A Solid Edge re-export of NIST CTC-01 that loses datum B's feature is pinned as a fixture.
- **Recognition (#178, #179):** through holes with partly obstructed exits, and helical coil springs. The schema fields are additive, but existing records change on affected parts: spring wire faces lose their hole, boss and fillet records, and partly covered exits become `through`.
- **Rust API:** `Features` gains the public field `helical_springs` and is not `#[non_exhaustive]`, so struct literals must supply it (`..Default::default()` still works). `PHYSICAL_FAMILIES` now has 43 entries. New public types: `HelicalSpring`, `SpringTolerances`, `ExitObstruction`, `GeometryCarrierExemption`.

### Fixed

- **Datum features that resolve to no faces** are reported as `Unresolved` findings: `datum <L> feature resolves to no faces` when the feature is kept but reaches no face, and `datum <L> is not read: what establishes it does not resolve` when nothing establishes the datum (previously a Nonconformance, "datum <L> is established by neither a feature nor a target"; such a datum was not read before either). The Solid Edge 2026 re-export of NIST CTC-01 and the control it was made from are fixtures (`tests/fixtures/ap242/reexport/`). The re-export reports exactly datum B unresolved. Writer round trips are checked so that no datum becomes unresolved.
- PMI dimensions and tolerances reuse the face's existing shape aspect and GISU,
  including datum features (CAx-IF PMI v4.1 §5.1 / §6.5.1–2). Location endpoints no
  longer use faceless `SHAPE_ASPECT('DIMENSION')` wrappers. Multi-face features
  retain composites of shared per-face carriers so OCCT reads one GISU per face
  without violating IIRU UR2. Adding a datum to a kept ordinary feature specializes
  that carrier to `datum_feature`, reported in `rewritten`, preserving its id and usages.
  **Consumer note:** written STEP entity linking changes; semantic PMI values and
  `haecceity.pmi/1` are unchanged. Reader binding advances to `haecceity-pmi-read/15`:
  explicitly authored standalone members use `SHAPE_ASPECT.name='haecceity.feature/1'`
  so they remain features when shared as composition members. Old files read as before.
  Specialized areas, runouts, groups and zones keep their meaning; plain requirements
  share neutral geometry carriers instead. Complex neutral composites can gain a datum
  in place. Legacy endpoint wrappers are retired on Add; independent same-content
  features retain separate removal provenance. A second datum label on an already
  established feature is refused under `datum_feature.WR1`.
  Adding a datum to a kept pattern specializes the same group aspect to a datum
  feature. Without presentation, adding a datum and its own size keeps the §6.5.3
  combined dimension over the kept geometry carrier, preserving existing feature controls.
  For ambiguous shared controlled-pair and datum-target-support SARs that both
  OCCT 7.9.3 XCAF and NIST SFA 5.41 read as composition, retain private forwarding
  aspects under a narrow §5.1 exemption.
  `WriteReport.geometry_carrier_exemptions` and the optional CLI/Python report field
  list each actual exempt addition by structural condition and endpoint/carrier id.
  Datum-feature endpoints stay direct; semantic face/GISU uniqueness remains mandatory.
  The thumbwheel Add golden retains one explicitly allow-listed presentation-only
  carrier so XCAF preserves the internal-thread callout, while coalescing its other
  producer duplicates. The raw Specify input is unchanged.
  Specify ownership stamps (`specify-core:written:v1`) are deleted with removed records;
  haecceity does not reattach them. Specify's own Replace re-stamps newly emitted roots
  after the haecceity write (`specify-core-rust/src/writer.rs:784`, `src/provenance.rs:91`).

### Added

- Recognise complete round-wire helical coil springs from swept NURBS tubes and connected cylinder/torus approximations. The optional `helical_springs` / `HelicalSpring` record supplies a file-space axis, hand, wire/mean/outer/inner diameters, physical free length, measured total coils, pitch, end type and exact claimed faces. Active coils remain null with an explicit basis when inactive ends are unproved; rate is never inferred. Spring claims suppress individual wire hole/boss/fillet callouts. Draftwright consumers can dimension free length and outer diameter on a simplified spring view and populate a data table from the record. Recognition schema 6 and fingerprint format 3 are unchanged for this additive family; Python recognition JSON exposes the same record.

- **Partially obstructed hole exits:** bores spanning their local wall now report
  `bottom: "through"` when a planar partial cap leaves part of the exit open.
  Optional `exit_obstructions` on hole records (also in pattern members) names
  each mouth, its covering face indices and approximate covered-area fraction.
  Full caps retain blind classifications; undecidable geometry remains `unknown`.
  Consumers should re-recognise affected parts, draw THRU and warn about the
  obstruction. Recognition schema `/6` and fingerprints `/3` are unchanged: the
  diagnostic field is additive, omitted when empty, and excluded from fingerprints.
  Rust `HoleRecord` literals must supply `exit_obstructions` (normally `Vec::new()`).

## 0.4.4 (2026-11-01)

Upgrading from 0.4.3: recognition schema `/6`, fingerprints `/3`, the `haecceity.pmi/1` schema and the PMI reader string `haecceity-pmi-read/14` are unchanged, so semantic PMI reads as in 0.4.3. STEP writing changes only for parts with threads. Presentation JSON gains the optional fields below, including for files without threads.
- **Thread callouts (#175):** written presentation now includes internal and external thread callouts (stated pitch, class and depth, or THRU) in a stack local to the feature, with any controls on that thread. Thread-free writes are unchanged.
- **Presentation JSON:** `haecceity.presentation/1` gains two optional fields so a host can redraw text in its own font. `polyline_roles` is per graphic. `cell_box` is per text placement: it appears on read file text when the file states a planar extent and a known alignment, and on generated text. Older documents still read. Readers that validate against the 0.4.3 schema (`additionalProperties: false`), or haecceity 0.4.3's JSON import, will reject the new fields. The Python package's schema copy is updated.
- **API note:** the Rust structs `json::presentation::Graphic` (`polyline_roles`), `json::presentation::TextPlacement` (`cell_box`) and `presentation::Callout` (`frame_strokes`, `text_symbols`, `cell_boxes`) gain public fields and are not `#[non_exhaustive]`. Code that builds them as struct literals must add the new fields; code that only reads them is unaffected. New public items: the function `presentation::thread` and the enum `json::presentation::PolylineRole` (not `#[non_exhaustive]`).

### Added

- Generate internal/external thread callouts with stated pitch, class and depth (or THRU), in feature-local stacks. Presentation JSON adds optional polyline roles and text cell boxes for host-font rendering; older documents remain readable. Five feature-on STEP goldens gain thread graphics: `thread_replaced`, `assembly_plate_pin_intent`, `editor_assembly_pin_alone`, `string_post_tapped_intent`, and `thumbwheel_thread_knurl_intent`; their XCAF captures are refreshed. Thread-free writes and feature-off goldens are unchanged.

## 0.4.3 (2026-11-01)

Upgrading from 0.4.2: recognition schema `/6`, fingerprints `/3` and the `haecceity.pmi/1` schema are unchanged; only files whose datum features have several constituents read differently.
- **Datum constituents (#173):** a face aspect composing a datum feature maps to the whole datum only when it is the datum's whole face set. Constituents of a multi-face datum (e.g. three separately sized lands) keep their own features and dimensions; the datum still holds all faces. Single-constituent datums (NIST STC10's seven folds, the Solid Edge CTC-01 re-export) read as in 0.4.2. Specify can drop its temporary LM10UU writer adjustment.
- **Bounded STEP strings (#172):** strings the writer generates are at most 8192 encoded bytes, so OpenCascade can read written files (NIST STC-10 failed before). Long descriptive values, including haecceity's JSON item-id map, are written as ordered lists of text chunks; legacy single strings still read. Readers of haecceity's ordering property or descriptive text outside haecceity must join the chunks in list order. Strings carried unchanged from the input are written as they are, with a non-fatal `long_strings` warning. Generated names, identifiers and fallback product/header names longer than the bound are now explicit errors (never truncated), and a generated note graphic whose wording exceeds it is named `General notes` (the full text stays in the chunked descriptive items). **API note:** both Rust `WriteReport`s (`#[non_exhaustive]`) gain `long_strings: Vec<LongStringWarning>` (`record`, `encoded_bytes`, `byte_offset`); the new public const `pmi::write::MAX_STRING_LITERAL_BYTES` is 8192; report JSON carries `long_strings` only when there are warnings.
- **Presentation (#171):** generated size callouts sit above their feature control frames in one annotation plane, and saved-view packing reserves label ink, so callouts no longer overlap. Written PMI graphics change accordingly.
- **PMI reader string:** changes from `haecceity-pmi-read/13` to `haecceity-pmi-read/14`.

### Fixed

- A plain face aspect composing a datum feature aliases the datum only when its face
  set equals the whole datum feature's. Constituents of a multi-land datum retain
  separate features and size dimensions; the datum still collects all faces once.
  Single-constituent datum features, all-around groups and per-member datum groups
  retain their existing semantics. **Consumers:** legacy PMI reader binding advances
  to `haecceity-pmi-read/14`; re-read bound PMI documents. Specify can remove its
  workaround for the three independent Ø19 sizes on LM10UU's datum A lands.
- STEP PMI writes bound generated encoded string literals to 8192 bytes, preventing
  OpenCascade's scanner overflow on NIST STC-10. Long descriptive values,
  including JSON item-id maps, UDA payloads and notes, use ordered compound/list
  representation items of text chunks; legacy single-string values still read.
  **Consumer note:** readers of haecceity's ordering property or descriptive text
  must concatenate marked chunks in list order. Semantic JSON is unchanged.
  Generated indivisible oversized names/identifiers are now explicit errors.
  Retained source/header/presentation text is preserved with non-fatal
  `long_strings` warnings naming the record and encoded length; both PMI and
  B-rep write reports expose the warnings (CLI and Python reports include them).
  Source product labels are preserved by B-rep writes; newly generated fallback
  product/header names remain bounded. Add also rechunks producer-owned legacy
  descriptive metadata at its original ids, preserving links and decoded values.
  **API note:** both Rust write reports gain `long_strings` (`LongStringWarning`), and
  `pmi::write::MAX_STRING_LITERAL_BYTES` is public; optional report JSON
  adds that field only when warnings exist. Existing semantic JSON is unchanged.
- Keep generated size callouts above their feature control frames in one annotation
  plane, and reserve plain label ink in saved-view packing to prevent overlapping
  callouts without loosening feature-distance or spindle ladder drift limits.

## 0.4.2 (2026-11-01)

Upgrading from 0.4.1: recognition schema `/6` and fingerprints `/3` are unchanged.
- **Recognition (#157, #159, #160):** turned steps on stock with proved spanner flats are kept, interrupted coaxial torus fillets are grouped, and chamfer/angled-step corners are omitted unless proven. At your next pin, affected parts' fillet records renumber their run-local ids where interrupted arcs merge (64 groups in the corpus), some `corner` values become null where they could not be proven, and turned parts with flats gain their turned steps.
- **PMI reading (#168):** strictly reversed dimension limits and signed deviations are read swapped, with a non-fatal finding, instead of refusing the dimension. Plain face aspects composing a datum feature resolve to that datum feature. On NIST STC10 seven duplicate features fold into their datum features (173 → 166), with references re-indexed.
- **PMI identifiers and zones (#170):** duplicate or empty source identifiers no longer abort the JSON read (NIST STC06 and STC09 now export); repeated ids fall back to content-derived JSON ids, with a non-fatal finding. Each `haecceity.pmi/1` item gains an optional `identifier` carrying the file's characteristic number, duplicates included. Update closed-object schemas for this additive field (the Python schema mirror includes it). Rust struct literals of `json::Semantic` and JSON-enabled `PartIdentifiers` need the new `identifiers` / `json_identifiers` fields. An exporter's runout zone with no angle on a non-runout tolerance recovers its stated zone form, with a finding.
- **PMI reader string:** changes from `haecceity-pmi-read/11` to `haecceity-pmi-read/13` (the `/12` step in #168 was never released).

### Fixed

- PMI JSON reads no longer abort on duplicate or empty source identifiers. Unique wire
  ids fall back to content identities, and non-fatal findings retain the affected STEP
  instances and identifier values. Optional per-item `identifier` fields carry source
  characteristic numbers, including duplicates, independently of reference `id` fields.
  **Consumers:** update closed-object schemas for this additive `haecceity.pmi/1` field;
  resolve references by `id`. The Python schema mirror includes the addition.
  Rust `json::Semantic` gains `identifiers`; JSON-enabled `PartIdentifiers` gains
  `json_identifiers` for round-trip source metadata, so struct literals need those fields.
- Recover an explicitly stated diameter/plain zone on a non-runout tolerance when an
  exporter supplies a runout-zone definition without its required orientation. Report
  the nonconformance; refuse when the missing orientation determines the zone's meaning.
  Legacy PMI read binding `/13` identifies the additional source-identifier findings.

- For #157, #159, #160, retain turned steps on stock with proved spanner flats,
  group interrupted coaxial torus fillets with exact defining faces, and omit
  chamfer/angled-step corners unless their wedge is consistent at published
  precision and has no material interior, retaining correct boundary-touching corners. Recognition /6 and fingerprints /3 are unchanged.
  **At your next pin:** affected parts' fillet records renumber their run-local ids where
  interrupted arcs merge (64 groups in the corpus), and some `corner` values become null
  where they could not be proven. Turned parts with flats gain their turned steps.
- PMI import repairs strictly reversed dimension limits and signed deviations with
  a non-fatal nonconformance finding identifying the source instances. Equal bounds
  and reversals whose nominal lies outside the corrected absolute limits remain refused.
- Plain face aspects composing an indivisible datum feature retain the owning feature
  identity when referenced by tolerances, properties or presentation; their faces are
  collected once. All-around groups and per-member datum groups keep their semantics.
  Legacy PMI reader binding `/12` identifies the corrected output.

## 0.4.1 (2026-10-31)

Upgrading from 0.4.0: recognition schema `/6` and fingerprints `/3` are unchanged, and successful recognition output is byte-identical.
- **Panic isolation (#161):** a panicking family no longer aborts the run. Incomplete documents add optional `failures` and `complete: false`.
- **Presentation:** text placements carry optional `stroked`; generated general notes are one numbered GENERAL NOTES block beside the part; regenerated ladders and datum flags sit by their features again; generated presentations gain one view per annotation plane. Feature-on (written PMI) graphics change accordingly.
- **Callout reading (#166):** `PmiRead.callouts` lists decoded graphical PMI annotations and their semantic links.
- **API note:** `haecceity::pmi::PmiRead` gains the public field `callouts`; code that builds `PmiRead` with a struct literal must add it. Code that receives it from `pmi::read` is unaffected.
- **AP214:** files with FreeCAD's `PRODUCT_TYPE` now take PMI (pzfreo/specify-core#89), with each keyword rewrite reported in `edition_rewrites`.

### Added

- Haecceity `PmiRead.callouts` lists decoded graphical PMI annotations and every
  DMIA semantic link, including unresolved definitions, independently of the existing
  presentation accounting count. Links use part-qualified model ids and follow JSON
  transport ordering; note callout reading remains compatible.

### Fixed

- Generated part notes share one numbered GENERAL NOTES block in JSON and STEP,
  retaining each requirement's semantic link and the bounds-relative placement.
- Regenerated PMI uses measured, compact dimension ladders and reserves datum
  flags before labels. Saved-view packing keeps labels near their features and
  text placements use the actual stroke transform, including clipped short spans.
  Written linear ladders share the compact ink spacing and retain seven-view clearance.
- For pzfreo/specify-core#89, write PMI into legacy AP214 files containing
  `PRODUCT_TYPE`, changing only that keyword to `PRODUCT_RELATED_PRODUCT_CATEGORY`
  and reporting each edition rewrite with its instance id and entity names.
- For #161, isolate panicking recognition families and block their dependents while
  preserving independent results. Incomplete recognition/6 and evidence/1 documents
  add optional `failures` and `complete: false`; successful output is unchanged.
- Haecceity presentation v1 adds optional per-text `stroked` metadata, avoiding
  duplicate generated labels, and orthographic views for generated annotation planes.
  Generated general notes now form a bounds-relative column two capital heights
  beside the part; written STEP notes use the same placement rule.

## 0.4.0 (2026-10-31)

Upgrading from 0.2.8: recognition schema `/6` and fingerprints `/3` are unchanged, and existing CLI output is unchanged.
- **Version jump:** the crates move from 0.2.8 to 0.4.0 so that they share one version with the new `quiddity` Python wheel, which replaces Python quiddity 0.3.x on PyPI. There is no 0.3.
- **Python:** `pip install quiddity` now gives the Rust-built wheel: a file-based API (STEP path or bytes in, JSON out) for CPython 3.11+. It does not take build123d/OCP shapes; keep the old API with `pip install "quiddity<0.4"`. See [docs/python.md](docs/python.md).
- **New:** `quiddity-rust/evidence/1` and `quiddity evidence`, and `quiddity::dimensioning` (#141 slice 1).

### Fixed

- Haecceity presentation surface-finish values now use the stated Decimal and unit,
  avoiding float noise such as `10.400000000000002` for `10.4` µm. Non-micrometre
  units are explicit; precision is retained in face symbols and general notes.
- Make the complete dimension prefix policy explicit and exhaustively tested:
  spherical `SR`/`S⌀`, circular and toroidal `R`/`⌀`, with no invented torus prefix
  (ISO 129-1:2018 §§7.2–7.4). Existing prefix output is unchanged.

### Added

- Python `pmi.write(..., report=True)` returns `(bytes, dict)` with verification
  differences, input/output reader findings, writer diagnostics and instance counts.
- The wheel now targets `abi3-py311` and CPython 3.11+, with CI smoke coverage on
  3.11 for all five platforms and on the newest CPython for Linux x86_64.
- In-process Python PMI interchange: STEP path/bytes plus a PMI dict/JSON string
  produce verified AP242 STEP bytes without temporary files. PMI/presentation reads
  return dicts, named/index anchors are selectable, `PmiError` preserves reasons and
  available findings, and both JSON Schemas ship as dicts and package resources.
  This replaces the provisional Python CLI-path wrappers; recognition is unchanged.
- A dependency-free `quiddity` 0.4.0 Python wheel (`crates/quiddity-py`), using
  PyO3 abi3 for CPython 3.11+, with STEP path/byte recognition, evidence, anchors,
  fingerprints, correspondence, capabilities, verified PMI JSON import/export,
  and the shared Rust CLI. This replaces the Python 0.3.11 API; see `docs/python.md`.
- Additive `quiddity-rust/evidence/1` JSON with records, defining/constituent,
  instance and host faces, source STEP identities and reconciliation candidates;
  `quiddity evidence <step> [--framed]`, `fingerprints` and `capabilities` commands.
- Python smoke/corpus tests and a five-platform wheel/sdist workflow. A published GitHub
  release (or a manual dispatch) publishes to PyPI after checking that the tag, wheel
  and crate versions agree; pushes never publish.
- Public `presentation::size_prefix` for drawing clients, and
  `presentation::label_lines_with_width` for independent, unpadded per-line geometry
  with caller-supplied ink widths. Existing padded labels retain their geometry.
- For #141, add `quiddity::dimensioning`: shared, view-independent requirements for
  prismatic holes, patterns and plane levels, with face-derived ids, datum origins,
  tolerance-aware consolidation and explicit conveyance/reference relationships.
  Step heights and tied shoulders replace raw interior plane intervals; later-slice
  feature planes and turned envelopes are explicitly deferred.

## 0.2.8 (2026-10-31)

Upgrading from 0.2.7: recognition schema `/6` and fingerprints `/3` are unchanged.
- **Feature split:** `json` is now serialise-only (PMI and presentation export). Use the new `json-import` for parsing, validation, the schemas and JSON → STEP. The CLI's `pmi json` commands need `--features json-import`.
- **Mass units:** both writers now write gram and milligram as conversion-based units of the SI kilogram (CAx-IF UDA Annex C.1), so written material densities change form. Old files still read.
- **OCCT densities:** the reader repairs OCCT's mass³·length² density and its untyped density value, with findings.
- **Presentation JSON** gains optional text placement, planes, graphic ids and saved views.

### Changed

- For #142, the `json` feature is now serialise-only (export of `haecceity.pmi/1` and `haecceity.presentation/1`, and id assignment). The new `json-import` feature adds parsing, validation, the packaged schemas and the JSON → STEP path. The CLI requires `json-import`. This keeps WASM consumers small.

### Fixed

- For #142, read OCCT's exact two-factor mass³ × length² material density as
  mass/length³, and tolerate its untyped REAL density value, reporting each repair.
  Other wrong dimensions remain refused. Writing emits the practice's typed
  density measure and correct exponents with conversion-based gram.

- For #142, encode integer/exponent Decimal spellings as valid STEP REALs while
  restoring their stated JSON text, including density exponents. Ordinary CLI
  verification accepts the required REAL spelling while retaining legal precision. Invalid density
  dimensions produce a finding. Both writers define gram/milligram as conversion-based
  units of SI kilogram per CAx-IF UDA Annex C.1/C.4.2; only affected mass-unit goldens change.
  Compact producer ordering metadata omits native defaults and uses ordinal references.
  Haecceity `json` now serializes without schemas/import; `json-import` adds those APIs,
  and the CLI requires `json-import`. Both crate levels use the same feature split.

- For #142, presentation JSON adds optional text placements, annotation planes,
  graphic ids and saved-view memberships while retaining schema `/1`. Datum-feature
  graphics also link to their owning datum, preserving feature aliases. Native
  text/placeholder positions and generated text metadata retain model-space placement.

- For #142, PMI JSON imports preserve stated target placement/direction decimals,
  including distinct tokens for equal REAL values. Dimension/tolerance-owned notes
  use direct AP242 characterized-item properties, upgrading edition 1 JSON writes
  to edition 4. Validation now shares geometry-independent writer refusals with import.

## 0.2.7 (2026-10-31)

Upgrading from 0.2.6: recognition schema `/6` and fingerprints `/3` are unchanged, and so is semantic PMI written by default builds. Changes:
- **New opt-in `json` feature:** PMI as JSON (`haecceity.pmi/1`, with the companion `haecceity.presentation/1`), JSON Schemas, and the `quiddity pmi json` CLI (build with `--features json`).
- **Presentation (on by default):** single-line dimension deviations are now drawn as `+upper/lower` (ISO 129-1:2004 §6.2); blank note lines draw no curve; the general-notes block and frame/diameter callouts are placed clear of each other in projected views. Written callout graphics therefore differ from 0.2.6.
- **New public API:** font-width-driven notation geometry (control frames, symbols, labels, surface texture, removal marks) and ordered fit-class lists, for drawing renderers.

### Added

- For #142, opt-in `json` exposes versioned AP242 semantic PMI and As Drawn/generated
  presentation JSON, packaged JSON Schemas, stable requirement ids and face-name/support
  anchors, plus `pmi json export`, `import`, `validate` and `presentation` CLI commands.
  V1 references use item ids; fallback ids derive from content. JSON writes preserve
  collection order, support projections, notes and attribute fields, keep foreign
  graphics for surviving requirements, and verify every item's id and content on read-back.
  JSON writes retain the existing AP242 refusals;
  complete source STEP closure remains blocked on the documented writer/source cases.

- For Draftwright #258/#269, public font-width-driven control frames, labels and surface
  texture symbols, GD&T symbol geometry/lookup, ISO 1302 removal marks and ordered fit-table
  discovery constants. Existing Hershey wrappers retain their sizing.

### Fixed

- Single-line asymmetric dimension bands now separate upper/lower deviations with `/`
  (ISO 129-1:2004 §6.2); diameter text retains `⌀`. ISO 129-1:2018 §6.2 drawing presentation
  uses stacked deviations or an ISO code; the README distinguishes that from plain text.
- Blank and whitespace-only general-note rows retain spacing without emitting empty
  tessellated curve sets. Specify can remove its `(blank)` workaround.

- Frame/diameter placement checks principal/isometric projections and uses corner-offset
  end-on frames for axial tolerances. The three Specify samples have no transverse stroke
  crossings between frames/diameters in all seven views; worst readable label/leader
  clearance is 0.391918359 mm (spindle). Measured witnesses and short-leader bounds remain.
- Correct stale optional-NIST tolerance-relation refusal pins against read/replace/read:
  FTC-08 retains all six relations; FTC-09 still loses dependent endpoints of refused groups.
  The writer already emits composite, precedence and simultaneity relationships.

## 0.2.6 (2026-10-30)

Upgrading from 0.2.5: recognition schema `/6` and fingerprints `/3` are unchanged. haecceity's
PMI writer now adds a tessellated callout (annotation plane, one semantic association) for every
dimension and tolerance it writes, ISO 1302 symbols for face surface textures, and one
`General notes` callout for part settings; build without the `presentation` feature for
semantic-only output. Written semantic PMI also changes shape: multi-face aspects are
`COMPOSITE_SHAPE_ASPECT`, location dimensions reference their own aspects rather than datum
features, and with presentation on a datum feature that is also a size is written as separate
`DATUM_FEATURE` and `DIMENSIONAL_SIZE`. Readers that key on face supports (as Specify does) are
unaffected; readers that key on entity structure should re-check. Files with no PMI written are
byte-identical. `PmiRead.identifiers` exposes each requirement's `id_attribute`.

### Added

- For draftwright-rust#254 and pzfreo/quiddity-rust#135, haecceity's PMI reader exposes
  each requirement's own AP242 `id_attribute` text in `PmiRead.identifiers`, parallel to
  provenance. Dimensions, tolerances, datums, surface textures (including part defaults),
  threads, knurls and other supported carriers retain their ids; duplicate attributes
  produce a finding instead of selecting a value. Lossless append preserves imported ids.

- For #137, default-on `haecceity::presentation` shares Draftwright's requirement notation,
  frame/symbol geometry and Hershey single-stroke lettering. Every newly written dimension and
  geometric tolerance has a styled tessellated callout, an annotation plane and one semantic
  association per callout in the part's shared presentation path. No-PMI bytes and appended
  foreign presentation are preserved. Planes sit beside their features with part-scaled text
  and a short deterministic stagger; linear dimensions have two measured witnesses,
  extension lines and arrowed dimension lines, shortest first on outward ladders in one
  shared plane per direction/side, with principal/isometric projection clearance.
  Multi-face aspects are composite; single-face datums own their GISUs as plain aspects.
  Locations use their own non-datum references. Datum
  leaders start inside their faces in planes containing the outward normal. A physical
  text-height floor and feature-scaled frame offsets improve fit-view legibility.
  Names carry full requirement notation; the notes block uses the tree title `General notes`
  while retaining its full drawn wording, and clears every callout’s complete drawn extent
  in principal/isometric projections. The feature
  can be disabled for semantic-only output. Face-specific surface textures get ISO 1302
  symbols, and written material, defaults, process settings and indexed general notes get
  one property-linked notes block per part. Shared wording/geometry comes from Draftwright;
  existing foreign requirements are not restated.

## 0.2.5 (2026-10-30)

Upgrading from 0.2.4: the schema stays `quiddity-rust/recognition/6` and fingerprints `/3`. A boss's
`height` (and its free-end `location`) now includes its own convex end chamfers and rounds, so
chamfered or rounded bosses report longer than before; a plate whose edges are chamfered or filleted
stays a plate; turned fillets on the inside of a corner now say `side: concave`. Fingerprint values
of affected bosses and plates change within `/3`.

### Fixed

- For #134, boss height and free-end location include adjoining proved end chamfers
  and rounds. Plate coverage and centres include the orthogonal projections of
  adjacent convex edge treatments, preserving real supporting plates under trimming
  without changing recognition thresholds. Supporting boss/plate fingerprints carry
  across those edits; matched plate skins seed face correspondence by their real
  planes. Genuine shaft shortening, plate thinning/removal and flat movement remain
  visible. Recognition schema `/6` and fingerprint format `/3` are unchanged; affected
  fingerprint values intentionally change within `/3`, as for #71 and #92. Saved
  `/3` fingerprints compared against new runs show height/position "adapted" on
  bosses with end treatments. End treatments require a convex end junction and
  boss-relative size bounds; concave turned fillets now report their actual side.

## 0.2.4 (2026-10-29)

Upgrading from 0.2.3: recognition and its schema (`quiddity-rust/recognition/6`, fingerprints
`/3`) are unchanged. Only haecceity's PMI writer changes: adding PMI to a file with location
dimensions now writes, where it was refused.

### Fixed

- haecceity: PMI added beside a file's location dimensions no longer breaks
  `item_identified_representation_usage.UR1`. A `dimensional_location` relating the feature it
  is measured from is not a composition of that feature, so the feature's usage is shared; and a
  face the file already identifies keeps its one usage whichever new feature would own it
  (#132, for pzfreo/specify-core-rust#63).

## 0.2.3 (2026-10-29)

Upgrading from 0.2.2: the schema stays `quiddity-rust/recognition/6` and fingerprints `/3`.
Three families are new (`turned_bodies`, `face_grooves`, `cylindrical_profiles`), so faces that
were unexplained now belong to records. Concave inside rounds are now `fillets` with
`side: concave`, and some former `blends` become concave fillets (#127). On a turned body,
a full-length OD cylinder is no longer reported as a `boss` (#123). Consumers that list every
family, or that expect the OD of a turned part as a boss, need updating.

### Added

- For #123, `turned_bodies` classifies complete analytic solids of revolution in
  any direction, including single-diameter parts, with OD, length, bores and an
  ordered meridian profile. Body records co-own every face with existing features;
  only a nearly full-length OD boss is suppressed on a classified body.
- For #123, `face_grooves` recognises annular recesses in planar faces with coaxial
  conical or cylindrical walls, in any axis direction and on prismatic hosts too.
  Records give rim diameters, depth, each wall angle and all floor/wall/treatment faces.
  Existing claims, including `annular_grooves`, retain precedence. The 608ZZ has two
  grooves, measured at the rim as ⌀18.5/⌀13 × 0.25 with approximately 45° walls.
  Recognition schema `/6` and fingerprint format `/3` are unchanged.

- For #127, `cylindrical_profiles` recognises otherwise unclaimed convex and concave
  cylindrical profile surfaces under 180° in any direction, with radius, axis, axis
  point, side, sweep and defining faces. Split patches of one connected surface share
  one record. Holes, bores, proved edge rounds and existing feature claims are excluded;
  concave radii below `min(2 mm, 5% of the smallest part-box dimension)` remain available
  for inside-round recognition. CTC-05's four R203.2 surfaces now carry profile records.
  Recognition schema `/6` and fingerprint format `/3` are unchanged.

### Changed

- For #127, recognise proved straight inside rounds as fillets with `side: concave`,
  including tangent cylindrical walls and spherical corner caps shared with straight or
  toroidal rounds. Preserve existing feature claims and the 135° edge-round limit.

## 0.2.2 (2026-10-28)

Upgrading from 0.2.1: the schema stays `quiddity-rust/recognition/6` and fingerprints `/3`, but
recognition output changes for two rules, so saved fingerprints of the affected holes and rounds
change. Holes: a near-side step deeper than 1.0 mm is now a counterbore, not a spotface, and a new
optional `outer_steps` field carries extra counterbore-class steps (#118). Rounds: straight convex
rounds between tangent walls are fillets at any wall angle and direction, so many former `blends`
become `fillets`, and a few false fillets are no longer fillets (#120). Rust code that builds
`HoleRecord` literals needs `outer_steps: Vec::new()`. The optional `f3d` feature is new and off by
default.

### Added

- Optional `f3d` feature in haecceity and quiddity: read visible Fusion solid geometry
  through cadmpeg 0.6.7, including placements, and use it for recognition, correspondence,
  parts, the JSON-lines service and new AP242 export with `brep --pmi`. Fusion has no imported
  PMI; `pmi read` returns an empty document and `pmi write` requires STEP input.

### Changed

- A near-side hole step is a spotface only at depth <= 1.0 mm and depth < 0.2 times
  its diameter, using the published rounded measurements; deeper functional recesses
  are counterbores. For #118. `cbore` keeps the innermost counterbore-class step;
  optional `outer_steps` retains all larger counterbore-class steps as `{diameter,
  depth}`, outermost first, and is absent when empty. Each depth is that cylinder's
  own mouth-to-floor span, not cumulative from the hole opening. Hole patterns and
  publication retain these steps and all their defining faces. Rust `HoleRecord`
  literals need `outer_steps: Vec::new()` when absent. Recognition schema `/6` and
  fingerprint format `/3` stay unchanged; affected hole fingerprints change traits
  and sizes, including outer-step count and paired dimensions within v3.

- For #120, a straight constant-radius convex round between two tangent walls is a fillet
  at any wall angle and in any direction, including drafted cast walls. Its cylindrical sweep
  must match 180° minus the interior wall angle within 0.5°;
  concave rounds and bores remain excluded. Oblique fillets carry unit-vector axes.
  Sweeps above 135° remain groove/slot full-round bottoms rather than edge fillets.
  Only tangent walls compete as supports; non-tangent transverse cut/end planes do not
  remove an otherwise proved round. Recognition schema 6 and fingerprint format 3 are unchanged.
  Whole-body classifications such as thin-wall and sheet-metal bodies retain their
  existing co-ownership of faces when a round becomes a fillet.
  Sheet-metal bend skins previously misread as fillets by the axis-aligned path's missing
  tangency check are no longer fillets; their sheet-metal classification remains.
  NIST CTC-05's four R203.2 cylinders with grossly non-tangent walls are also no longer
  fillets. CGB243 face 196, an R6 half-round with a 180-degree sweep, is no longer a
  fillet: it exceeds the 135-degree edge-round cap and remains unexplained.
  Support tangency respects the STEP file's modelling precision, bounded below
  by the kernel coordinate floor. Accepted defining claims take precedence over new
  fillets, retaining the existing gusset and circular-pattern claims on rounded faces.

### Fixed

- Recognition no longer panics on disconnected pocket wall cycles or narrow pad tops;
  wall roles use measured geometry and refused discovery evidence remains visible in documents.

## 0.2.1 (2026-10-27)

Upgrading from 0.2.0: as for 0.2.1-alpha.1 below. Upgrading from 0.2.1-alpha.1: no library
change; the schema stays `quiddity-rust/recognition/6` and fingerprints `/3`.

- **Snapshot tool in Rust; leaner CI.** `tools/rust_snapshot.py` is replaced by the unpublished
  workspace crate `snapshot-tool` (`cargo run -p snapshot-tool --release -- [--check | --diff REF]`),
  with the same provenance, clean-tree and CI rules. CI no longer replays the snapshots before the
  shards: the Linux shards record what they verify and one job checks coverage and provenance.
  Port-era and duplicated test work is removed (caller-space comparisons, recognition of
  `captured/` fixtures in the transform and publication sweeps, 4,200 legacy per-function corpus
  replays, repeated unmoved readings). Pull requests run the full suite on Linux and only the
  float-sensitive pins on macOS; main, manual runs and `release*` PRs run the full macOS suite.

## 0.2.1-alpha.1 (2026-10-26)

Upgrading from 0.2.0: the schema stays `quiddity-rust/recognition/6` and fingerprints `/3`.

- **New family:** `rectangular_through_notches`. Readers that list families must accept it.
- **`haecceity::step::StepError` has a new `Limit` variant.** The enum is exhaustive, so a
  `match` on it needs an arm for `Limit`.
- **Malformed assemblies are refused whole.** An assembly cycle or nesting past depth 64
  refuses the file instead of returning a partial part.
- **Re-recognised records change fingerprints:** CTC-01's notches, rounded V-channels and
  polygonal boss caps, and the 14 files whose frame changes (below).

- **CTC-01's last unexplained faces (for #100).** `v_channels` also proves a rounded
  trough: two rectangular flanks joined by one concave cylinder tangent to both, with
  `trough_radius` and `depth`/`at` from the flanks' intersection; sharp-trough records are
  unchanged. New `rectangular_through_notches` records a U cut through a recognised plate's
  edge: bottom and two facing side walls each spanning exactly the slab on the plate faces'
  outer loops, sharp or equally rounded concave corners, and convex mouths on one outward
  plane; corner rounds stay blends, and a full-span channel keeps its walls. A polygonal
  boss claims its consulted free-end cap when it lies within the side ring. Schema stays
  recognition `/6`, fingerprints `/3`; boss fingerprints change faces, centroid and
  adjacency. CTC-01 face accounting is now 97 feature, 20 body, 0 unexplained (was
  90/20/7): flanks 11/13/15/17 are `v_channels/0,1`, notch walls 31–41 are
  `rectangular_through_notches/0,1` (31, 35, 37, 41 keep their step-level evidence too),
  and 104 joins `polygonal_bosses/0`. The synthetic fixture `plate_edge_notches.step` is
  generated by `tools/make_plate_edge_notches_fixture.py`. A notch is dropped with its plate
  when reconciliation rejects that plate.

- **haecceity refuses malformed and hostile STEP instead of panicking (for #52).**
  Library code no longer `unwrap`s, `expect`s, `panic!`s or reaches `unreachable!`
  except at a site with a targeted `allow` stating its invariant (clippy enforces it);
  the rest became typed refusals or fallbacks. Recursive and input-sized read paths
  are bounded (`haecceity::step::limits`; new `StepError::Limit`): surface-curve
  wrappers 16 deep (a cyclic chain used to hang), assembly depth 64 (now refused rather
  than cut off), 100 000 instances, 500 000 faces, 10 000 edges per bound, knot vectors
  no longer expanded past their pole count, PMI shape aspects 16 deep, and an iterative
  Douglas–Peucker. An assembly cycle or nesting past depth 64 now refuses the whole file
  where it used to return a partial part. Face integrals past 2^20 panels are left
  undetermined rather than coarsened. No fixture or corpus file hits a bound.
  `tests/robustness.rs` reads hostile files on a 1 MiB stack and sweeps truncated and
  byte-flipped fixtures, querying (mesh, mass, HLR) the mutants that still read.
- **Oracle ledger (for #52).** `crates/haecceity/tests/oracle_ledger.json` records,
  per kernel query, the cases agreed with each independent reference, the pinned
  differences by verdict and the deferred ones with what they wait on; its test fails
  when an agreed count falls or a deferral disappears unresolved.

- **A part's roll follows its own outline (for #91).** The frame vote still settles the
  leading line, but a less-voted basis about that line now sets the roll when the box around
  the material (every edge's sampled points) is more than 1e-4 smaller; an equal vote keeps
  the file-axis tie rule, and the box never reads the file's axes, so a rigid motion still
  carries the frame. Of 2,959 corpus and fixture STEP files, 14 change frame:
  `golden_oriented_slots` and its five captures (slots 14 votes to the plate's 6, box 0.53)
  and the 17-degree slotted assembly's four captures (10 to 8, box 0.44) return to the
  plate's file axes, so each slot claims all four walls and the plate faces are body;
  cgb241 and its capture return to file axes (20 to 18, box 0.94), so face 47 is again a
  convex R23 blend (its walls' normals are 92.06 degrees apart) and the two oblique-plane
  step levels go; cgb217 turns to the 13.07-degree set of its cross bores and largest walls
  (93 votes, box 0.80 of the 45-degree set's), so none of its sixteen R1 rounds runs along
  an axis and all are blends, with no tilted fillets; its cross bores now lie on frame axes,
  so the `InteriorVoidRefusal` over faces [26, 28, 41] (an incomplete two-wall sampled
  component in the 45-degree grid) is no longer emitted. Schema stays recognition `/6`,
  fingerprints `/3`; fingerprints of the re-recognised records on those parts change.

## 0.2.0 (2026-10-26)

Upgrading from 0.1.1: recognition schema 6 and the API changes in 0.2.0-alpha.1 and
0.2.0-alpha.2 below. Re-recognise rather than reuse 0.1.1 output.

Upgrading from 0.2.0-alpha.2: no schema or API break. The schema stays
`quiddity-rust/recognition/6` and fingerprints `/3`.

- **New families:** `annular_grooves`, `semicircular_blind_slots` and `v_channels`. Readers that
  list families must accept them.
- **More faces claimed:** drill-point holes claim their tip cones and rectangular slots their end
  walls, so those records' fingerprint values change; `quiddity correspond` pairs them.
- **Face accounting** gains the body reasons `plate_edge` and `stock_wall`, and blended box
  corners now count as stock (CTC-01: 86/0/31 feature/body/unexplained becomes 90/20/7).
- **Pockets** gain `rectangular_boundary`, which defaults to false when absent.

- **Conservative stock-wall support (for #100).** Plate edges also check parallel
  walls outside their slab over the same run, so a step shoulder beside a thin
  plate strip remains unexplained. Interior stock walls must be perpendicular to
  the plate and have complete, perpendicular original-stock boundary edges along
  their run and height; parallel cut ends and walls borrowing two stock end caps
  abstain. Walls entirely on an outward envelope plane retain their independent
  supporting-plane evidence and convex stock contacts.

- **Face accounting and record coverage (for #100).** Recognised convex corner
  treatments can supply replaced box corners; certified enclosed spline trims on
  native surfaces no longer veto stock evidence. New `plate_edge` and `stock_wall`
  reasons require plate perimeter/support and direct convex stock anchors, reject
  concave cut boundaries, and keep plate-only notches unexplained. Drill-point
  holes claim their terminal cones; rounded rectangular slots claim both proved
  end walls after reconciliation, preserving feature selection and the inventory
  floor. Schema remains recognition `/6`, fingerprints `/3`; affected feature
  fingerprints change faces, centroid and adjacency, with correspondence retained.
  CTC-01 has 90 feature, 20 body and 7 unexplained faces (formerly 86/0/31).
  The issue omitted four V-notch flanks and incorrectly assumed cap 104 was already
  claimed by `polygonal_bosses`; those stay unexplained. Four rectangular-notch
  side walls retain their existing step-level claims. No notch or boss cap is body.
- **Prove rectangular pocket boundaries before class scoring.** Publish `rectangular_boundary`
  only for an intact straight rectangular floor with four full-height inward planar walls,
  uniquely owned cavity evidence and no extra treatment faces. Circular-end pockets and blind
  corner steps exclude that proved closed rectangle; unclassified boundaries remain ambiguous.
  Against mapping round 2 alone on 2,000 dev models, rectangular-pocket instance recall/precision
  improve 0.1946/0.1539 to 0.8452/0.6678, circular-end 0.2782/0.1382 to 0.7910/0.6138, and
  blind-step 0.7850/0.3919 to 0.7850/0.6141. Their face recall/precision improve respectively
  0.4477/0.7492 to 0.7030/0.8216, 0.4358/0.4696 to 0.6392/0.7689, and
  0.7567/0.4762 to 0.7567/0.6869; no class loses true positives.
  Pocket fingerprints gain the invariant `rectangular_boundary` trait within format v3.

- **Complete pocket boundaries past intersecting mouths.** A uniquely owned intact rectangular
  floor can prove both observed end walls after a crossing cut opens the mouth wire. Existing
  cavity-emptiness and role proofs remain required. The 2,000-model dev set gains one more
  rectangular-pocket TP without extra predictions: instance recall/precision
  0.8438/0.1934 to 0.8452/0.1937 (594 to 595 TPs).

- **Recover rectangular pockets from intact floors.** A closed straight-edged floor with a
  concave wall on every edge now supports four-sided sections too, retaining all cavity,
  floor and opening volume proofs. The 2,000-model dev set gains one rectangular-pocket TP:
  instance recall/precision 0.8423/0.1932 to 0.8438/0.1934 (593 to 594 TPs).

- **Complete pocket wall evidence.** A uniquely owned cavity now retains its observed planar
  end walls or the sides smoothly joining both obround caps as defining faces; floors remain
  context and ambiguous ownership keeps the seed unchanged. On the 2,000-model MFInstSeg dev
  set, rectangular-pocket instance recall/precision rise from 0.1946/0.0446 to 0.8423/0.1932
  (137 to 593 TPs), and circular-end-pocket from 0.2782/0.0985 to 0.7910/0.2799
  (185 to 526 TPs), with unchanged prediction counts and no model/class TP losses.

- **Subdivided oblique-step boundaries.** Merge only collinear outer-edge vertices and
  complete straight seam intervals; retain gaps, bends and non-quadrilateral outlines
  as refusals. MFInstSeg dev slanted-step instance recall 302/683 → 318/683
  (0.4422 → 0.4656), face recall 604/1447 → 636/1447 (0.4174 → 0.4395);
  both precisions remain 1.0000.

- **Circular steps opening on shoulders.** Accept a local convex opening only with
  outward perpendicular terminals, incident orthogonal corner supports and an empty
  terminal sweep. Ambiguous axial ends remain refused. MFInstSeg dev circular-step
  instance recall 499/666 → 530/666 (0.7492 → 0.7958), face recall
  1000/1377 → 1062/1377 (0.7262 → 0.7712); both precisions remain 1.0000.

- **Angled-step blind-cap evidence.** Concave triangular caps perpendicular to the run
  define the cut; convex stock terminals remain context. MFInstSeg dev triangular-step
  instance recall 426/643 → 458/643 (0.6625 → 0.7123), precision 0.8894 → 0.9562;
  face recall 458/1345 → 915/1345 (0.3405 → 0.6803), precision 0.9562 → 0.9744.

- **Paired ramps in short stock.** Establish the valley axis from its seam and convex
  opening / concave blind terminal, independently of the stock aspect ratio. An alternate
  full-run concave sidewall path closes a pocket and remains refused. MFInstSeg dev
  two-sided-step instance recall 339/650 → 519/650 (0.5215 → 0.7985), face recall
  1020/2065 → 1560/2065 (0.4939 → 0.7554); both precisions remain 1.0000.

- **Drilled oblique-step walls.** Check the rectangular outer boundaries independently
  of inner drill loops, retaining convex joins and the exact empty-sweep proof. MFInstSeg
  dev slanted-step instance recall 271/683 → 302/683 (0.3968 → 0.4422),
  face recall 542/1447 → 604/1447 (0.3746 → 0.4174); both precisions remain 1.0000.

- **Shallow oblique through steps.** Require principal supports to have zero transverse
  normal components within angular precision, so shallow slanted walls retain their
  oblique role. MFInstSeg dev slanted-step instance recall 137/683 → 271/683
  (0.2006 → 0.3968), face recall 274/1447 → 542/1447 (0.1894 → 0.3746);
  instance and face precision remain 1.0000.
- **Lip-bevelled blind-slot scoring.** Existing `section_recesses` can prove a
  rectangular U sweep whose mouth has one flared planar bevel. Map only its complete
  straight five-vertex profile, one planar blind cap, one open run end, and a run
  strictly longer than both nominal section dimensions beyond publication uncertainty.
  On 2,000 dev models, rectangular_blind_slot instance recall/precision improve from
  0.4758/0.9833 to 0.4919/0.9839 (59 to 61 TP). Face recall/precision change from
  0.4618/0.9833 to 0.4736/0.9758 (236 to 242 TP): the existing defining bevel faces
  remain predictions, while cap context is not promoted. Both extra instance
  predictions are correct. No class loses TP; no engine rule or failure count changes.

- **Conservative geometric subtype scoring.** Compare published straight section
  coordinates at their declared precision against complete triangles, rectangles and
  regular hexagons; exclude only a proved alternative canonical profile. Retain treated,
  intersected or unresolved profiles under the old correspondence. Blind-hole scoring
  excludes proved through ends but retains unknown ends. Pocket scoring excludes proved
  incompatible corner-notch or half-cylindrical-end subtypes without treating an absent
  flag as proof of another type. On 2,000 dev models, every class/model retains all face
  and instance TP and recognition failures remain zero. Per-class recall/precision:

  | Class | Instance recall/precision, before → after | Face recall/precision, before → after |
  |---|---|---|
  | triangular_passage | 0.8028/0.3478 → 0.8028/0.8937 | 0.7938/0.2492 → 0.7938/0.8461 |
  | rectangular_passage | 0.6547/0.2878 → 0.6547/0.8953 | 0.6473/0.2740 → 0.6473/0.8720 |
  | 6sides_passage | 0.7682/0.3437 → 0.7682/0.9152 | 0.7526/0.4900 → 0.7526/0.9398 |
  | blind_hole | 0.9789/0.3450 → 0.9789/0.5074 | 0.5018/0.3510 → 0.5018/0.5129 |
  | triangular_pocket | 0.8903/0.4635 → 0.8903/0.9262 | 0.6627/0.3098 → 0.6627/0.8776 |
  | rectangular_pocket | 0.1946/0.0446 → 0.1946/0.1539 | 0.4477/0.1577 → 0.4477/0.7492 |
  | 6sides_pocket | 0.8484/0.4559 → 0.8484/0.9443 | 0.7312/0.6107 → 0.7312/0.9494 |
  | circular_end_pocket | 0.2782/0.0985 → 0.2782/0.1382 | 0.4358/0.3092 → 0.4358/0.4696 |
  | rectangular_blind_step | 0.7850/0.2778 → 0.7850/0.3919 | 0.7567/0.3305 → 0.7567/0.4762 |

  Through-hole correspondence remains coarse: one flat-bottom record still contains a
  through-labelled face. No engine rule changes and no label selects a prediction.

- **Internal-round scoring correspondence.** MFInstSeg `round` labels include internal
  fillets: all 158 existing concave blend defining faces on dev are round-labelled.
  Extend the published `blends.side` selector to convex or concave, retaining the
  existing proof and leaving #89's convex-only `fillets` rule unchanged. Relative to
  convex-blend scoring, round instance recall/precision improve from 0.6373/0.9543 to
  0.9017/0.9638 (376 to 532 TP), and face recall/precision from 0.5617/0.9975 to
  0.7858/0.9982 (396 to 554 TP). No class loses TP or gains recognition failures.

- **Convex blend scoring.** Map the round class to already-proved `blends` with
  published `side = convex`, deduplicating their defining-face sets against `fillets`.
  No fillet radius floor or material-side rule changes. On 2,000 dev models, round
  instance recall/precision improve from 0.2831/0.9176 to 0.6373/0.9543 (167 to 376 TP),
  and face recall/precision from 0.2567/0.9945 to 0.5617/0.9975 (181 to 396 TP).
  No class loses TP and no new recognition failures occur.

- **Complete chamfer scoring correspondence.** Map internal `oriented_chamfers` as
  well as `chamfers`, matching their existing merged recognition-schema-6 publication;
  identical defining-face sets remain deduplicated. On 2,000 dev models, chamfer
  instance recall/precision improve from 0.4033/0.6967 to 0.5223/0.7343 (271 to 351 TP),
  and face recall/precision from 0.3862/0.7943 to 0.4975/0.8326 (309 to 398 TP).
  Recognition, other class scores and failure counts are unchanged.

- **Validation worker limits.** Snapshot and MFInstSeg baseline tools honour exported
  Cargo/test worker limits, and the baseline scorer honours `MFI_WORKERS`; defaults and
  all regression refusals are unchanged. Recognition scores are unchanged.

- **Annular floor/rim partitions.** Smooth coplanar floor pieces and split circular
  edges retain one annular groove when their two closed boundary cycles cover full,
  concentric circles; overlapping arc length alone is not proof. All floor pieces
  define the feature, and strict solid/mouth evidence remains required. On 2,000 dev
  models, Oring instance recall/precision improve from 0.8624/1.0000 to 0.8640/1.0000
  (520 to 521 TP); face recall/precision improve from 0.8301/1.0000 to 0.8317/1.0000
  (1,563 to 1,566 TP). No other class loses TP or gains failures.

- **Semicircular-bottom blind slots.** Publish `semicircular_blind_slots` for a complete
  half-cylinder tangent to two rectangular sides, with one planar blind cap and proved
  open run/lateral mouths on a strictly valid solid. Map MFInstSeg
  `v_circular_end_blind_slot` to this geometry instead of the quarter-cylinder/flat-floor
  family used for its horizontal counterpart. On 2,000 dev models, instance
  recall/precision rise from 0/0 to 0.7971/0.9821 (110/138 TP); face recall/precision
  rise from 0/0 to 0.7885/0.9821 (440/558 TP). No other class loses TP or gains failures.

- **V-section through channels.** Publish `v_channels` for two complete rectangular
  planar walls meeting concavely, with coplanar open lips and two open run ends, on a
  strictly valid solid. Wall pairs contained in a proved closed passage belong to that
  passage, rather than a separate channel; passage records are unchanged. Map
  MFInstSeg `triangular_through_slot` to this family. On 2,000 dev models, instance
  recall/precision rise from 0/0 to 0.3836/0.9180 (56/146 TP); face recall/precision
  rise from 0/0 to 0.3463/0.9508 (116/335 TP). No other class loses TP or gains failures.

- **Circular through-channel scoring.** MFInstSeg mapping schema 2 permits selectors over
  published record fields, with geometric evidence for each selector. Map
  `circular_through_slot` to existing `section_recesses` classified as `channel` and
  `circular`; recognition is unchanged. On 2,000 dev models, instance recall/precision
  rise from 0/0 to 0.5488/0.9474 (90/164 TP); face recall/precision rise from 0/0 to
  0.4577/0.9583 (92/201 TP). No other class loses TP and there are no new failures.

- **Annular face grooves.** Recognise a planar annular floor between complete coaxial
  cylindrical walls terminating at one open plane, with strict valid-solid evidence.
  Publish `annular_grooves` in recognition schema 6 and map MFInstSeg Oring to its floor and two
  defining cylindrical walls. On the 2,000-model dev set, Oring instance recall/precision rise from
  0/0 to 0.8624/1.0000 (520/603 TP); face recall/precision rise from 0/0 to
  0.8301/1.0000 (1,563/1,883 TP). Other classes and recognition failures are unchanged.

## 0.2.0-alpha.2 (2026-10-24)

Upgrading from 0.2.0-alpha.1:

- **`oriented_slots`, `oriented_chamfers` and `oriented_slot_patterns` are gone** from the
  published document: their records are in `slots`, `chamfers` and `slot_patterns`, whatever
  the recognition frame. Feature IDs of formerly oriented records change; the schema stays
  `quiddity-rust/recognition/6` and fingerprints `/3`. See [docs/recognition-6.md](docs/recognition-6.md).
- **API.** The typed `Deref` view of `PublishedFeatures` is removed: use `families()` or
  serialisation.

- **Slot precision in turned frames (for #92).** Converted slots retain measured
  extents instead of rounding distances to 0.01 mm. Slot patterns use raw member
  centres for pitches and directions, with coordinate-floor axis snapping and
  face-stable array sense. File-axis slot extents keep their existing values.

- **One published family per feature kind (for #92).** Recognition schema 6 merges
  oriented slots, chamfers and slot patterns into their plain families, preserving
  proofs and counts. Plain records precede oriented records; IDs, pattern members,
  evidence and accounting follow the merge. Fingerprints stay version 3, but merged
  records' fingerprint values change within v3: family/ID, oriented chamfer `turned`,
  and oriented slot source-end proof traits. As with plates in #71, the version is
  unchanged. Correspondence accepts 0.1.1's oriented records and retains their old IDs
  in the pairs. See [recognition-6.md](docs/recognition-6.md) for field mapping and
  moving saved references by defining faces or correspondence. File-axis letters
  and the crate version remain unchanged.

## 0.2.0-alpha.1 (2026-10-24)

Upgrading from 0.1.1:

- **Recognition schema 6 (#92).** Published records have no `local` fields. An axis field
  (`axis`, `long_axis`, `width_axis`, `other_axis`, ...) is a file-axis letter as before, or a
  unit vector in file coordinates; a converted record carries an `origin` and its measuring
  axes. Only records that had `local` fields change; the top-level `frame` is provenance only.
  Readers must accept `quiddity-rust/recognition/6`: see [docs/recognition-6.md](docs/recognition-6.md).
  Fingerprints stay `quiddity-rust/fingerprint/3`.
- **API.** `Recognition.features` and `Mapped` use `PublishedFeatures`; `Mapped::local`,
  `fingerprinted_local` and `LocalFields` are removed. Plate `lo`/`hi` are `Placed<f64>` (#84).
- **Changed results.** Frames change on parts whose file axes win the new vote (#81): CTC-02
  and FTC-07 AP242 become axis-aligned, cgb217 and cgb241 change frame. Concave rounds are no
  longer convex fillets (#89). CTC-02's 49.5 mm rows are no longer a pattern (#82).
  Re-recognise rather than reuse 0.1.1 output.

- **Publish nonprincipal measurements in file coordinates (for #92).** Recognition schema 6
  removes `local`. Formerly frame-relative directions use unit vectors; relative coordinates
  have explicit origins and measuring axes. Ordinary file-axis records and intrinsic
  fingerprints retain their values. Recognition frame voting and oriented families are unchanged.

- **Decide fillet convexity from the face (for #89).** Both prismatic paths now require
  an outward-facing cylinder. cgb241 face 27 returns to its concave R20 blend: fillets
  fall from 2 to 1, with convex face 47 retained. cgb202 loses 17 inward-facing false
  fillets (296 to 279) and recovers 3 concave blends (59 to 62); CTC-03 loses 6
  inward-facing false fillets (17 to 11) and recovers 5 blends (10 to 15).
  CTC-02 rc/AP242 keep 21/20 fillets; FTC-07 rd/AP242 keep 16/16. The raw inventory
  also drops 13 concave candidates on 12 MFCAD++ parts, 21 on 13 holdout parts,
  21 on 18 captured fixtures, and 1 on golden_circular_blind_step. Every removal
  has per-face material-side evidence; no outward-facing fillet is removed.

- **Preserve tiny material rotations during recognition (for #81).** File-axis support
  preference keeps the original near-axis material representative; publication alone snaps
  within the coordinate floor. sm-hanger again retains its 2.42e-9-radian inferred tilt while
  publishing in file axes, and a 1e-6-radian copy retains all 6 blind pockets and its pattern.

- **Frame inference votes over complete direction sets (for #81).** Planar normals and
  cylinder axes each vote once per distinct coplanar plane or coaxial equal-radius cylinder; file axes win a tie, while genuinely rotated parts retain
  their intrinsic frame. Real 1° features no longer merge into the first direction read.
  CTC-02 AP242 fillets rise from 3 to 20 (rc has 21: its extra R1.093 feature is absent
  from AP242); FTC-07 AP242 fillets rise from 13 to 16 and polygonal bosses from 0 to 4,
  with all 5 plates retained. cgb202 fillets rise from 272 to 296 and cgb217 from 6 to 14;
  cgb207 recovers its missing file-axis fillet at a total of 20. All 43 Specify fillet
  regressions are recovered. Parallel chamfer support edge directions are compared without endpoint-order dependence
  after frame normalisation, recovering mfcadpp/10252's missing 18.694-mm, 0.54×45°
  chamfer (0 to 1), the 44th Specify regression. CTC-02 AP242 also gains 4 pockets (0 to 4)
  and step levels (2 to 13); its 4 former risers were outer envelope walls seen in the wrong
  frame, and are gone. FTC-07 AP242 step levels rise from 6 to 8. Mixed-direction parts can
  change frame and so directional families: cgb217 moves from 13.1° to 45° (98 against 96
  supports) and cgb241 from the file axes to 27.4° (20 against 18). On cgb241 that relabels
  two blends (faces 27 and 47) as fillets; face 27, a concave R20, is a blend again after
  #89, leaving only face 47 relabelled. Counting distinct surfaces rather than faces keeps
  mfcadpp_regressions/11512 aligned and unchanged despite split channel faces (15
  file-axis surfaces against 10 rotated). Measured analytic evidence accompanies every bounded framed-floor allowance; broader
  shallow-support fillet coverage remains a limitation.
- Plate endpoints are published once on the file's 0.001 mm grid, with exact face
  separation retained for fingerprints (for #84). Their `lo`/`hi` fields now use
  `Placed<f64>`, as other rounded positions do, retaining the original planes through
  subsequent motions. Nearby projecting details no longer
  extend a predominant plate skin within the opposing skin's transverse envelope:
  whistle_frame_reference is 7.0 mm thick again (for #85). The six supporting-plane
  corrections from #71 remain in place; only whistle's plate fingerprint thickness changes.
- **CTC-02's 49.5 mm rows are not published as a pattern (for #82).** Its six ø8.38
  holes at x = ±25, ±74, ±122 are spaced 48, 49, 50, 49, 48. Alpha.8 chose one of two
  mirror-image three-hole rows (spacings 49 and 50, mean 49.5) by input order; both share
  the two central holes and have equal support, so under ADR 0011 neither is published.
  No refusal record is emitted; every hole is still recognised, and the 13 other patterns
  remain. A regression test pins the part and its hole stations.
- **Identical countersinks at opposite bore mouths stay attached (for #83).** The ø6 hole
  in draftwright-rust's two-face fixture again links to its ø14, 90°, 4 mm deep seat;
  both countersink records remain (2). A through hole retains its symmetric seats and
  both sets of supporting faces. Its `csink` refers to the seat at the presentation end;
  competing seats at one mouth still refuse.

## 0.1.1 (2026-10-22)

Upgrading from 0.1.0:

- **Record position fields are `features::placed::Placed<T>` (#69).** A field that holds a
  rounded position (a hole's `location`, a fillet's `at`, a blend's `center`, and so on) is now
  `Placed<T>`. It dereferences, compares and serialises as the published value, so reading it
  and recognition documents are unchanged; code that builds records or destructures such a
  field takes the value with `*` and builds one with `Placed::exact` or `.into()`.
- **Changed results.** Six plates' `lo`/`hi` and fingerprint `thickness` change (#71; the
  fingerprint version stays `quiddity-rust/fingerprint/3`); CTC-04 loses its 12 false
  countersinks (#76); framed positions move by less than one grid step (#69); sm-hanger's
  records are published in file axes (#70). Re-recognise rather than reuse 0.1.0 output for
  these parts.

- **Chamfer corners are no longer countersinks (for #76).** A countersink's cone must now run
  at least 270° round its axis (all faces on that cone together). On NIST CTC-04 the 12 reported
  countersinks were the quarter-cone corner faces of the 5×45° pocket-rim chamfer, seated only on
  the pockets' R5 corner-radius walls; they are gone, and the corner faces are now `unexplained`
  in the face accounting (no record spans the rim round its corners). No other corpus part
  changes; whole cones over a bore are kept.
- **A join within one surface on the outline is judged as the silhouette it is (for #75).**
  Hidden-line projection judged a seam or same-surface join lying on the view's outline from
  the edge's curve, which a file may leave a hair inside the surface: NIST CTC-05's shaft
  outline (edge 375, 1.5e-8 inside its cylinder) sent the ray along the surface into the
  cylinder's other half, and the front view drew it hidden. Such an outline edge is now judged
  from 1e-5 of the part's size outside its first drawn face, as traced silhouettes are. Across
  the corpus only outline stretches change, in 11 parts: of 594 mm now drawn visible, 392 mm
  already lay under a visible line, 190.5 mm is CTC-05's shaft, 8.9 mm is in cgb203's side view
  (closer to OpenCascade) and under 1 mm is in each of six other parts.
- **CTC-05 is in the OpenCascade hidden-line comparison (for #75).** `tools/capture_hlr.py`
  keeps CTC-05 although its record exceeds the size limit; its four views agree with
  OpenCascade. Its edge 226 ends on a vertex 0.018 mm off its circle's plane, so the edge's
  exact circle strays up to that far from the stretch's points; the comparison allows that gap
  for CTC-05 only (`references/drawings.json`), and the circle is still drawn exactly.
- **A compound's child order does not change recognition; a test now pins it (for #73).** The
  1 channel against 2 in #73 is the file: draftwright-rust's forward-order `two-channels.step`
  holds one occurrence of the block, the reversed file both.
- **Faster STEP reading (for #72).** The inverted-shell repair (#59) no longer integrates
  every face of every solid on each read. It first looks for one face with an outward witness,
  by ray parity against boxes that certainly hold each face (no face interior sampled), which
  proves the shell is not inverted. The full test (sampled boxes, signed volume, every face
  inward, wire senses) runs only when that proof is not found. The same shells are repaired,
  with the same reports, part definitions and masses. Instructions retired, v0.1.0 → now
  (alpha.8 for reference), for `read_step` and `read_part_definitions`: nist_ctc_02 10.3G →
  0.9G (0.88G); nist_ctc_01 0.55G → 0.21G (0.20G); gramel string_post 0.90G → 0.27G (0.13G).
  string_post is still about twice alpha.8: the remaining cost is in the proof's rays, which
  build the B-spline surfaces' grids and edge deviations that recognition needs anyway, so a
  read that never recognises (such as a PMI-only read) still pays it.
  `read_part_definitions` still builds the B-rep and runs the repair: the PMI edge numbering
  depends on the repaired face senses, so skipping it would renumber edges on inverted inputs.
- **A plate's `lo` and `hi` are again faces of the part (for #71).** Since #50 a side whose faces
  lie on several nearby planes published the midpoint of their face band, a plane no face lies
  on (GRM-03's thumbwheel screw: thickness 2.25 between faces 2.5 apart). The midpoint still
  chooses the band; `lo` is now the lowest of the −axis side's planes and `hi` the highest of the
  +axis side's, unrounded, which exchange correctly when the part is reversed. Six of the
  corpus's 48 plates change, with their fingerprint `thickness`; the fingerprint version stays
  `quiddity-rust/fingerprint/3`, whose meaning (thickness is `hi − lo`) is unchanged.
- **Framed records are on their published grids (for #69).** A position a recogniser rounds
  (a fillet's `at` to 0.001 mm, a hole's location to 11 significant figures, a recess's
  `w_center` to 0.01 mm, a body key's box to 1e-6 mm) now keeps the value it was rounded
  from. Framed recognition carries that unrounded value through the frame's motion and rounds
  it once, in the file's coordinates, on the same grid; a decimal half-way point within the
  coordinate floor takes the even neighbour. Positions derived from published ones (pattern
  centres, a gusset mirror plane, an oriented slot's centre) are derived again from the
  published values. Values move by less than one grid step: NIST CTC 01's hole at
  `-124.99999999647548` is `-125.0`. Section recesses publish their frame origin on the
  plane through the file's origin across the run, with run coordinates measured from it.
  Sizes, counts, orders and fingerprints are unchanged (fingerprints read the values as found
  in the frame). Record fields that hold positions are `features::placed::Placed<T>`, which
  dereferences to the published value; build one with `Placed::exact` or `.into()`.
  Positions a recogniser publishes unrounded (a plate's faces, a boss's location, step and
  riser levels, sheet-metal and thin-wall bodies, freeform poles, a hole pattern's centre as
  the mean of its members) are carried unrounded, as before. Each end of an interval is
  rounded in the file on its own, so `hi - lo` can differ from the published size by one grid
  step where it did not before (and agree where it did not): read sizes from the size
  fields, not from interval ends.
- **No noise near zero in significant-figure positions (for #69).** A hole's `location` (11
  significant figures) and a circular blind step's points (6) kept round-off near zero, such
  as NIST CTC 01's holes at z = 4.3e-11. A coordinate within the kernel's coordinate floor
  (`COORD_FLOOR`, 1e-6 mm) of zero now publishes as 0. Other values keep their significant
  figures: hole patterns are found from the published locations, and rounding those to
  0.001 mm loses patterns.
- **Near-aligned frames are published in the file's axes (for #70).** Recognition still runs
  in the frame as inferred. Its records are published through the signed permutation of the
  file's axes nearest the frame when that moves no point of the part by more than the
  kernel's coordinate floor (`COORD_FLOOR`, 1e-6 mm; the rotation's distance from the
  permutation times the part's reach from the frame origin), so they map to the file's axes
  exactly instead of staying frame-local, and differ from the exact motion's by less than
  the floor. On `sm-hanger.step` (2.4e-9 rad off) the six fillets, the plate and the body
  keys are now in the file's axes; only step levels, read along the frame's z by
  specification, stay local. Recognition and fingerprints are unchanged: fingerprints read
  the records carried through the exact motion.

## 0.1.0 (2026-10-22)

Upgrading from alpha.9:

- **Recognition API (#61).** `correspondence::recognise(&Part)` is the one entry point and
  returns framed records: on a part that is not axis-aligned in its file, read `local`
  fields through `frame`. `recognise_placed` is gone, and caller-space
  algorithms are private: `features::recognise`, `recognise_<family>`, discovery and
  helper functions, `features::context::Context`, `recess_reduce::merge_proposals`, the
  reader-based `frames` helpers (`prepare_framed*`, `working_part`, `FramedPart`,
  `FramedRecognition`, `FramedError`) and `evidence_view::build_recognition_evidence`.
  Feature record types stay public. `RecordFrame::CallerSpace` exists only in crate tests,
  so drop it from exhaustive matches. The `python-parity` feature is removed (#67). `read_step_placed` and `read_step_file_placed` refuse
  non-orthonormal and non-finite placements.
- **Recognition documents are `quiddity-rust/recognition/5` (#66)** with a `face_accounting`
  array: every face is feature, refused, body or unexplained. Versions 3 and 4 still read,
  with accounting `None`. `Recognition` gains `input_repairs` and a private field, so it
  can no longer be built as a literal. `RecognitionDocument` gains `face_accounting` and
  `input_repairs` (#59, #66).
- **Records may differ from alpha.9's (#57, #58).** Placement- and order-dependent choices
  were corrected, and a few readings are now refused (ADR 0011).
  Fingerprints stay `quiddity-rust/fingerprint/3`, but re-recognise rather than reuse
  stored IDs, faces or dimensions.
- **haecceity uses step-io 0.3 (#62).** Part 21 strings are decoded text, spans are `u64`,
  and schema names are `schema.raw`.
- **PMI (#60).** Reader binding `/11`. `Zone` has a new `affected_plane_kind` field, and
  `WriteReport` is `#[non_exhaustive]`, and `PartProvenance` gains `note_callouts`. JSON
  documents stay version 2.
- **License.** Both crates are Apache-2.0 (#55).

New in 0.1.0: the B-rep STEP writer and `quiddity brep [--pmi]` (#64, #65), PARABOLA geometry
(#62), the inverted-shell repair (#59), revolution volumes (#56), and a regression record that
Rust owns, with Python no longer consulted (#67).

### Added

- Complete recognition face accounting: feature record IDs, named refusals and reasons,
  positively established body faces (`stock_box_side`, `stock_box_notched`,
  `stock_box_split`), or unexplained. The conservative
  geometry rules and independent MFInstSeg ratchet are in [docs/face-accounting.md](docs/face-accounting.md).
  No feature record or fingerprint changes.
  **Upgrading:** all newly emitted recognition documents, including `quiddity serve`, use
  `quiddity-rust/recognition/5` and include a `face_accounting` array indexed by file face.
  Read the group on each face; do not interpret missing accounting on older documents as
  an empty unexplained list. Versions 3/4 still deserialize with accounting `None`.
- A B-rep STEP writer, `haecceity::brep_write::write_step`: a loaded `Part` (every solid,
  voids included, and faces in no solid; every surface and curve kind the reader holds,
  NURBS, parabolas, hyperbolas and swept surfaces included; pcurves; orientation; placed
  instances in components placed as the file placed them) written as a new AP242 file through
  step-io 0.3's `StepBuilder`, deterministic, with its semantic PMI carried through
  `pmi::write` and every item not carried named in the report. Reading the written file gives
  the same part bit for bit and the same recognition; geometry that did not resolve is refused
  by name; source product names are kept. `quiddity brep <file.step> -o <out.step>` writes a
  file's part with its PMI, or with `--pmi <pmi.json>` with the PMI of a `quiddity-pmi`
  document bound to that file (edit PMI as JSON, write it with the B-rep to a new file);
  `tools/check_brep_write_occt.py` checks written files against OpenCascade. Design, guarantee
  and refusals: [docs/brep-writer.md](docs/brep-writer.md). For files we were given, PMI edits
  stay byte-preserving (`pmi write`).
- `pmi::without`: a part's PMI less some items and everything that refers to them (moved from
  the PMI round-trip test). `Part::file_uncertainty`: the source file's length uncertainty.

- Read parabola edges and faces bounded by them. Like hyperbolas, finite parabola spans
  use exact quadratic B-splines for evaluation, sampling, bounds and mass/area integration;
  rigid transforms retain their native conic geometry. OCCT trim points take precedence
  over conflicting parabola parameters; parameter-only trims use ISO 10303-42's
  dimensionless parameter `(F*u*u, 2*F*u)` because no point establishes a writer's convention.

- AP242 PMI writing of composite, precedence and simultaneity tolerance relations; stated
  default-tolerance table cells and decimal places within them; material density in its own
  dimensionally correct derived unit; edition 4 relationship targets and affected-plane links.
- Legacy manufacturing-requirement notes migrate to semantic text or unambiguous surface
  texture. `standard_form` exposes the expected requirements and the CLI write report adds
  `note_migrations`. Replace removes legacy properties and their owned presentation. The reader
  binding holds affected planes and table qualifiers and separates relation targets from
  feature composition; JSON document version 2 stays compatible.
- Regression fixtures for the new forms, five ordered numbered part notes across replace and
  resumed add, and Python specify-note migration. Literal target and standalone-decimal
  schema/practice conflicts use the reported exemptions authorized on 2026-10-20 (evidence
  in `step-ap242.md`).

- Write every model datum-target shape and the datums it establishes (PMI practice §6.6),
  and decimal-only Figure 3 tolerance cells/defaults. `practice_schema_conflicts` reports the
  exact added instance/rule exemptions authorized by the maintainer (target WR5 only, decimal
  cell WR3/WR4). Structural validation and every other rule remain mandatory; raw rules and
  NIST file-wrong pins are unchanged. Reader binding `/11` projects anonymous decimal-only
  carriers to part defaults and keeps semantic text on targets separate from parameters.
- Synthetic target/precision fixtures, the 13 CTC-02/FTC-10 target clean-copy round trips,
  and OCCT cross-checks including shared target supports; no target-refusal stripping.

### Changed

- Rust owns its regression baseline, with source-commit provenance and deterministic gzip
  expectations. Regenerate with `python3 tools/rust_snapshot.py`, review decompressed changes
  with `--diff BASE_REF`, and verify current HEAD with `--check`. Python is no longer consulted.
- Removed Python recognition/PMI oracle outputs, capture-only tools, parity harnesses and the
  `python-parity` Cargo feature. Caller-space algorithms are private; their internal tests
  retain independent reference checks. The MFInstSeg scorer compiles the shared private engine
  without a feature flag, preserving its scoring semantics and label floors. Python-written inputs, live OCCT checks, NIST/design-intent,
  analytic, label, motion and inventory-floor tests stay. Independent ledgers live under
  `tests/fixtures/references/`; retired verdicts are archived with evidence and commits in
  `docs/history/python-parity.md`.

- Haecceity now uses step-io 0.3, including its read-only model API, streaming readers,
  typed conics and shell/face/bound/edge-use `Orientation`. Haecceity's B-rep orientation
  flags and public geometry API remain unchanged. Hyperbola spans retain the existing
  arithmetic, using step-io's typed geometry as their source.
  **Public Part 21 API:** `p21::Attribute::String` and `Document::graph()` now contain
  decoded Unicode text. Pass plain text to `Attribute::String`; `p21::encode` escapes
  it on output. `p21::escape`/`decode` still convert encoded string bodies, and must
  not be applied to already decoded attributes. Re-exported parser spans use `u64`
  byte offsets, as in step-io 0.3. The raw graph's schema names are now the
  `schema.raw: Vec<String>` field (formerly `schema.raw()`), and its upstream
  `Graph` type is non-exhaustive, so callers cannot construct it with a struct literal.

- One supported recognition entry point: `correspondence::recognise(&Part)` recognises in the
  inferred part frame using an in-memory rigid transform, without a reader closure or second
  STEP read. Haecceity's `Part::transformed` preserves topology and source indices and rebuilds
  derived caches lazily; non-rigid placements and reflections are refused.
  **Upgrading:** `recognise` now returns framed records. Some fields are `local` on parts not
  axis-aligned in their file; consumers must read them through `frame`. `recognise_placed` is
  gone. Caller-space `features::recognise` and `recognise_<family>` are no longer public in
  builds. Discovery, inventories, context construction and caller-space helpers are private;
  feature record types remain public. Internal unit tests exercise these algorithms without
  a feature flag. The old reader-based frame helpers and `RecordFrame::CallerSpace` exist
  only in crate tests.
  `Part::transformed` is new public haecceity API. Both `read_step_placed` and
  `read_step_file_placed` now also reject non-orthonormal and non-finite placements, in
  addition to reflections.

- Chamfers, fillets and circular blind steps recognise perpendicular supporting planes rolled
  about a principal run axis, using the planes’ geometry. Motion tests and tighter breakdown
  limits cover the recovered features; the default recognition route is unchanged.
- Recognition no longer depends on a part's placement or on input order where ADR 0011 says it
  must not: every first-wins, sign or ordering selector was audited and corrected (holes and
  counterbore stacks, bosses, recesses and channels, plane corners, patterns, turned axes,
  circular patterns, thin walls, voids, coordinate clusters, frames, oriented chamfers). A
  remaining arbitrary choice is a presentation gauge, never a semantic one; only two readings
  that would publish different features are refused. Identical counterbores at both ends of a
  hole keep their dimensions and claim both mouths. A per-file, per-family inventory floor over
  1,453 parts guards against losing features; the few changed counts each carry OpenCascade or
  analytic evidence.
- **Recess proposal merging (`recess_reduce::merge_proposals`, now private) refuses
  equal-width role ties with different run axes when no body extents are available.**
  Previously input order selected an interpretation. Part-aware recognition uses intrinsic
  body extents to accept a unique reading; symmetric competing claims remain refused
  (ADR 0011). The document schema is unchanged.
- quiddity-rust and haecceity are relicensed from AGPL-3.0-only to Apache-2.0 by the copyright
  owner, matching Python quiddity. `LICENSE`, `NOTICE`, the CLA and CONTRIBUTING say so.
- Both crates are ready for crates.io: descriptions, repository, keywords and categories, an
  `include` list (sources, README, CHANGELOG, LICENSE, NOTICE; no test fixtures), and an exact
  version on quiddity's `haecceity` dependency. `cargo package --workspace` packages and
  verifies both (haecceity 520 KiB, quiddity 505 KiB compressed).

### Fixed

- Repair consistently inward closed solid boundaries on STEP import, reporting each
  affected solid in input repairs and recognition warnings. Mixed or unproved
  orientations remain unchanged (#53).
- Restore regression coverage for original-input note indices in refusals, reporting
  semantic-text and surface-texture requirements on unwritable features by their own items.
  Narrow datum-target exemptions to WR5; WR4 remains mandatory because every emitted
  target name is empty.

- Isolate relationship tolerance endpoints so OCCT composition traversal leaves other
  tolerances on their original faces. Correct the earlier projection-origin diagnosis.
- Distinguish affected intersection line/plane and orientation plane descriptions (§6.9.2.1)
  with optional `Zone.affected_plane_kind`; ambiguous variants are refused. The JSON
  addition is optional. Rust literals of `Zone` require the new field.
- Keep unitless legacy roughness as semantic text rather than invent micrometres. Report
  every supplied note callout and link removed callout ids/types to their original note in
  `note_presentation_removed`. `WriteReport` is now `#[non_exhaustive]`.
- Check and name tolerance relationship cycles as `geometric_tolerance_relationship.WR3`.
  Clarify that omitting a target description passes rules but loses its shape. OCCT
  affected-plane verdicts are not-applicable.

- Preserve a single sheet when converting radial `SURFACE_OF_REVOLUTION` line
  profiles, and recover boundary inversion stalled at a collapsed surface side.
  This corrects revolution face fluxes and mass properties without discarding
  their prescribed orientation (#53).

## 0.1.0-alpha.9 (2026-10-20)

Upgrading: fingerprints are now `quiddity-rust/fingerprint/3` and `correspond` refuses
version 2, so stored alpha.8 fingerprints must be regenerated from the STEP file (#51).
Feature IDs keep their `family/n` form and most are unchanged, but IDs are positional and
some records are added, removed, reordered or claim different faces (see Changed), and many
published sizes move. Re-recognise rather than reuse stored alpha.8 IDs, faces or dimensions.

### Added

- Add recognition document trust metadata: exact input-file SHA256, completed inventory
  mode, and strict per-record evidence status. Patterns inherit member proof; source
  refusals remain unproved. The fields are additive: documents keep their 3/4 schema tags and
  existing records, but a `/3` document is no longer byte-identical to alpha.8's; readers must
  ignore unknown fields. See [the trust contract](docs/recognition-trust.md) (#28–#30).
- `InteriorVoidRefusal` records: a bounded-air candidate whose sampled widening behind a throat
  is not established, previously dropped silently, is kept in the document's `interior_voids`
  family after the recognised voids, with `reason: "widening_behind_throat_unproved"` and its
  complete measurements. Its `faces` are the candidate's skin; it is `unproved`, claims no
  recognised feature, has no fingerprint and does not appear in the top-level
  `interior_voids` array. A document containing one is tagged `quiddity-rust/recognition/4`.
  `recognition::RECORD_TYPES` lists it under `interior_voids`. `Record::is_refusal()` covers
  it, so CLI and `serve` warnings do not list it as an unproved record
  ([docs/recognition-4.md](docs/recognition-4.md),
  [docs/recognition-trust.md](docs/recognition-trust.md), #49).
- New API: `recognition::{EvidenceStatus, RunMode}`, `Record::is_refusal`,
  `RecognitionDocument::{source_sha256, run_mode}`, `Record::evidence_status`,
  `Recognition::source_sha256`, `Features::{run_mode, evidence_status,
  interior_void_refusals}`, `features::interior_voids::InteriorVoidRefusal`,
  `features::evidence::strict_status`, `features::policy::round_measurement`; in haecceity, `step::decode_step_file_bytes` and
  `FaceMoments::second_moments` (central second moments). Code constructing these structs
  literally must supply the new fields (#28, #29, #33, #46, #49).
- Rust-first independent-reference policy (README, CONTRIBUTING): the Python captures are a
  frozen regression record; new behaviour is judged against OpenCascade, analytic fixtures,
  MFInstSeg/MFCAD++ labels and motion invariance (#22).
- Labelled-dataset accuracy: `examples/mfinstseg_score.rs` scores face and instance
  precision/recall against MFInstSeg (or MFCAD++ with `--mfcadpp`) through the source-cited
  [class mapping](tests/fixtures/mfinstseg_mapping.json); `examples/face_order.rs` and
  `tools/check_dataset_face_order.py` verify face order against OpenCascade; `tests/mfinstseg.rs`
  ratchets a hash-pinned 200-model sample (runs only with `QUIDDITY_MFINSTSEG` set; dataset not
  committed), updated with `tools/update_mfinstseg_baseline.py` (#23–#25).
- Evidence tools: `tools/check_face_integrals.py` with `examples/face_integrals.rs` (live
  OpenCascade face integrals and an independent 3D-boundary integration), and
  `tools/centroid_evidence.py`, `tools/perimeter_evidence.py`, `tools/drawing_evidence.py` with
  `examples/centroid.rs` and `examples/trim_lengths.rs` (boundary-flux centroids, raw STEP trim
  lengths, 3D silhouettes) (#35, #41, #43).

### Changed

- Fingerprint version 3 (#51): sizes are measured from raw geometry before positions are
  rounded, with decimal half-way points within the coordinate floor rounded ties-to-even
  (e.g. 92.075 → 92.08; #46); equal pocket wall separations keep all walls and choose by face
  identity, so square-footprint pockets now claim all four walls (#45); a hole stack with equally
  wide distinct ends opens at the end its cylinder's own axis points to, not the end high in
  world space (#47); groove supports are found symmetrically at both floor ends, and a proved helical thread root (cgb243 face 224) is no
  longer also published as a groove (#48); void sampling uses a box-centred lattice, which
  changes sample counts and volume estimates (#49); plate face bands use the midpoint of their
  extreme planes at full precision, which changes `lo`, `hi` and thickness (#50). Coordinate
  clustering and level-riser tests now include the tolerance boundary (#45). See
  [docs/correspondence.md](docs/correspondence.md).
- Records that change on existing parts include: mfcadpp_regressions/11512's chamber on faces
  33–34 becomes an `InteriorVoidRefusal`; a GRM-03 riser on the level-tolerance boundary is
  no longer published; nist_ctc_03's two slots change order (#45, #46, #49).
- Under motion, the invariance ledger's `rust-wrong` entries fall from 22 to 0 and the
  correspondence ledger's from 53 to 2; both remaining are mfcadpp/10170's section-recess
  `run_interval` endpoints on the three-decimal grid (#45–#50).
- Face integration controls error on every component (area, first and second moments) in
  dimensionless coordinates. Stored areas, centroids and framed coordinates move by at most
  3e-8 relative on the 30 quality parts; nist_ctc_05's framed pocket length on the 60.325 tie
  moves between 60.32 and 60.33 (#33).
- haecceity `face_mass` and `face_moments` return `None` for a non-finite or non-positive
  integral instead of a zero or `abs()` value (#33).
- Parity ledgers: undetermined verdicts fall from 113 to 2 (#36, #42, #44), settled with
  OpenCascade, independent boundary integration, MFCAD++ labels and motion evidence; the two
  left are cgb242 face 483's area and moments. Settling them changes only verdicts and reasons.
- CLI and `serve` warnings now read `<path>: warning: <reasons>; retained N records; unproved
  records [ids]`. The reasons are unresolved geometry, no solid (shell geometry only), solids
  failing strict validity, a local-degradation retry, or unproved record evidence. Refusal
  records are excluded from the unproved list. A refused section-recess projection keeps its
  own `section-recess projection refused: <reason>` warning (#29).
- The frozen per-platform section-recess publication hashes are replaced by an in-run check
  that publication is additive to the same recognition without it (#33).

### Fixed

- cgb242's solid centroid is no longer refused after translation (#33).
- Correspondence anchors skip a face without an area instead of weighting it as zero (#34).

## 0.1.0-alpha.8 (2026-10-20)

### Added

- Recognise modelled (helical) screw threads (#20). A `modelled_threads` record carries its axis, side (internal or external), major and minor diameter, pitch, hand,
  number of starts and extent along the axis, and claims its flank, crest, root and runout faces
  with stated tolerances. Inconsistent geometry (e.g. varying pitch, unequal starts, missing
  opposed flanks) becomes a `modelled_thread_refusals` record with a machine-readable `reason`,
  claiming no faces; no ISO designation is inferred. Across the 100-part corpus five threads are found, on cgb243 (four)
  and threaded_connector_109 (one); unrecognised freeform faces fall from 74 to 66 and from 140
  to 4 on those parts, and the other 98 outputs are byte-identical. A diamond knurl and a
  round-wire spring are not recognised as threads; a shallow helical groove and a varying-pitch
  sweep are explicitly refused. See [docs/modelled-threads.md](docs/modelled-threads.md).
- New API: `analyse_modelled_threads`, `recognise_modelled_threads`, `ModelledThread`,
  `ModelledThreadRefusal`, `ThreadAnalysis`, `ThreadAxis`, `ThreadTolerances`; `Features` gains
  `modelled_threads` and `modelled_thread_refusals`; `evidence_view::PHYSICAL_FAMILIES` grows
  to 41 entries (#20).
- Recognition documents are `quiddity-rust/recognition/4` only when they contain thread records
  or refusals; all others stay `/3` byte for byte. `recognition::SCHEMA_VERSION` names `/4`;
  compare with `LEGACY_SCHEMA_VERSION` or `schema_version(&features)` (#20).

### Changed

- `haecceity::pmi::Accounting::presentation` counts graphic PMI only: annotation occurrences,
  draughting callouts, and the draughting models and presentation views that carry them.
  Geometry styling, colours, curve fonts, layers and shared representation contexts no longer
  count, so a geometry-only file reports no PMI presentation (e.g. NIST CTC-01 392 → 0; the
  CTC-01 AP242 file 843 → 71). The reader binding moves to `haecceity-pmi-read/8`; the JSON form
  stays version 2, and PMI JSON bound to `haecceity-pmi-read/7` is refused by `pmi check` and
  `pmi write`; re-run `quiddity pmi read` (#21).

## 0.1.0-alpha.7 (2026-10-18)

### Added

- Derive semantic PMI axes from coaxial revolution faces, median planes from two parallel
  planar faces, and a plane from a single planar face. Cross-check explicit constructive
  geometry: a point agrees with an axis when it lies on the axis or on a deriving surface.
  Retain deriving face indices and cylinder radius, and report unsupported or disagreeing
  geometry precisely. Dimension and tolerance targets can reference these records;
  qualifiers remain data.
- PMI document format 2 (`haecceity-pmi-read/7`): optional `derived.geometry` references
  the new `derived_geometry` list. See [the record format](docs/pmi-derived-geometry.md).
  The shared format version also moves the `quiddity parts` listing and write reports to 2.
  Existing version 1 documents must be regenerated from STEP.
- GRM-03 (a Gramel thumbwheel drive screw): all ten semantic dimensions now resolve to
  geometry (previously all refused), with nominals and qualifiers unchanged. NIST AP242
  output is unchanged; see [the integration evidence](docs/pmi-derived-evidence.md).

## 0.1.0-alpha.6 (2026-10-18)

### Changed

- Prism emptiness probes (used by native slot and recess recognition) now ask
  haecceity's bounded `fills_at_most(.., 0.0)` whether the prism holds any material,
  stopping at the first positive running volume, instead of computing the complete
  common volume (#16). Results are unchanged: unresolved-line semantics are kept, and a
  box that rounding collapses at extreme coordinates keeps the previous fraction-valued
  answer. In draftwright-rust#90, which combined this change with draftwright-side
  optimisations, planning NIST FTC-10's AP242 file fell from 203 s to about 5 s (most of
  the 203 s was spent in this probe), and the 65-part plan-time survey used 29% fewer
  instructions, with every plan JSON and SVG byte-identical.

## 0.1.0-alpha.5 (2026-10-17)

### Added

- Recognition document `quiddity-rust/recognition/3` retains Python-shaped
  `section_recesses` occurrences and source refusals, and now publishes
  `section_recess_patterns` (`SectionRecessArray`, `SectionRecessGrid`). Members are
  occurrence indices into `section_recesses`, never source-pocket indices. Pattern
  faces are the union of members' defining faces; frame mapping carries directions
  and the grid centre. Step levels and risers were already published in version 2.
  Existing record payloads and the K3 local-degradation behavior are preserved.

## 0.1.0-alpha.4 (2026-10-16)

### Added

- `PartDefinition.product_id`: the part's `PRODUCT.id`, escapes decoded, reported beside its
  index and display name (assemblies design D1).
- Tests on the assembly fixture downstream uses: a part read in its own coordinates, one part's
  PMI written alone (the other part's instances unchanged) with OpenCascade's reading of the
  result, and a nested sub-assembly flattened to its leaf parts.
- Python's local-degradation retry (`_take_inventory`): where the strict hole evidence refuses
  for want of one valid solid on a part whose solids are not all valid, `features::recognise`,
  `features::inventory`, `section_recess_projection` and `build_recognition_evidence` run again
  with every validity query answering as Python's degraded graph does, and holes, pockets,
  fillets, plates, step levels, risers, rectangular pads, slots, channels, prismatic pockets and
  edge-open recesses skip what Python skips there (`plates::discover_locally_degraded` is new). Corpus parts 13975 and 14052 now publish
  Python's section recesses (none; 14052's triangular pocket) instead of carrying the
  projection's refusal, and their inventories and evidence views match Python's.
- `RayCaster::hits_on` and `RayCaster::other_hit`: the hits on some faces alone, and whether a
  face other than some meets a ray within a stretch, answered without intersecting the other
  faces wherever that gives the same answer as `hits` (`None` otherwise).

### Changed

- Hidden-line projection is faster on parts whose faces stack up along the view (a thread seen
  along its axis, a comb's teeth): threaded_connector_109 renders in 4.1 rather than 23.5
  trillion instructions, cgb243 in 2.7 (draftwright #43 item 3). Every drawing is unchanged:
  each saving gives the same answer by construction. A stretch's visibility asks about only the
  faces it needs (`hits_on`, `other_hit`), skips asking its edge's own faces again when the ray
  is stopped past them anyway, and reuses a B-spline face's hits along a ray it has already
  asked; rays meet nearer boxes first; a B-spline face's grid cell is tested against its padded
  box before its triangles; a basis and its derivative come from one pass; a face domain's
  segment bins leave out segments parallel to the parity ray and stop below the ray's start.

## 0.1.0-alpha.3 (2026-10-14)

### Added

- Upgrading an AP203 or AP214 file to AP242 to add PMI now repairs two closed classes of
  defect that are also invalid in the file's own AP203 edition 2 schema, so six NIST AP203 files
  (CTC-01, CTC-02, CTC-03, CTC-05, FTC-06, FTC-09) take PMI: a `CC_DESIGN_APPROVAL` of a product
  definition becomes `APPLIED_APPROVAL_ASSIGNMENT`, and an inch or degree `NAMED_UNIT(*)` gets
  explicit `DIMENSIONAL_EXPONENTS` from its conversion factor's unit. Any other invalid
  instance is still refused. `WriteReport.repairs` and the CLI JSON report each repaired id, its
  kind and its original and new record text; every other byte is unchanged. Removing the PMI
  later does not restore the repaired records' original bytes (`docs/step-ap242.md` decision 1).
- `QUIDDITY_CORPUS=off` skips the corpus tests for a quick local run.

### Fixed

- haecceity reads hyperbola edges exactly, as one rational quadratic arc between the edge's
  vertices, when both vertices lie on the curve. Such edges were previously unresolved and their
  solids invalid, which turned off recognition on them (155 MFInstSeg models contain one).

### Changed

- The correspondence-under-motion check runs inside `tests/invariance.rs` on the same
  recognitions as the invariance check; the duplicate `cycle_xyz` corpus motion is dropped.
- The section-recess publication byte baseline is referenced on Linux, with the macOS values
  as alternatives.
- CI runs on pushes to `main`, now the only branch. Contributions are covered by a contributor
  licence agreement (`CLA.md`).

## 0.1.0-alpha.2 (2026-10-11)

### Added

- Publish `section_recesses` in `Features` JSON and as `SectionRecess` records in the
  recognition document consumed by draftwright (maintainer decision, 2026-10-11). Each record
  keeps Python's `index`, `body`, `geometry`, `classification` and `evidence`, its projection
  order and defining faces, and the existing caller/file coordinate mapping and `local` labels.
  Native recesses, uncovered prismatic pockets, passages, edge-open recesses, blind slots,
  pockets and channels use the existing reconciled projection; overlapping regions are
  published once. Existing source families and records are retained byte-for-byte.
- Schema note: this is an additive family under `quiddity-rust/recognition/2`, as for step
  levels and risers; the schema version is unchanged. Readers that validate a family
  allowlist must add `section_recesses`, `SectionRecess` and `SectionRecessRefusal` before
  updating its tag pin (the G7b reader stream). Refusals follow occurrences under the same
  family, preserving Python's source evidence and order for downstream lint warnings. Derived
  patterns remain available through the dedicated section-recess API.
  Corpus document tests compare occurrences and refusals on all 30 draftwright quality parts
  with captured Python records
  and preserve the existing verdicts for 15 incorrectly open NIST channels.

### Fixed

- Face mass no longer hangs on boundaries where the kink detector finds a new kink after every
  split (MFInstSeg 20221123_142528_16681): the kink-split budget is shared by the interval, so
  such a face now refuses (no area) instead of never returning.

## 0.1.0-alpha.1 (2026-10-10)

The first alpha: a pure-Rust port of Python quiddity's feature recognition, on haecceity, a
B-rep kernel and STEP/AP242 library that replaces OpenCascade.

### Added

- **haecceity kernel** (`crates/haecceity`, re-exported as `quiddity::kernel`): B-rep from STEP
  (via `step-io`) with face and edge provenance by `#N`; exact surfaces and curves (analytic,
  NURBS), face moments, volumes, rays and point classification, per-face geometric validity,
  hidden-line and section views for drawings (`hlr`), face triangulation (`Part::mesh`, `Part::triangulate`), a point on each
  trimmed face (`Part::face_anchor`) and the common area of two coplanar faces
  (`overlap::common_area`).
- **STEP and AP242 reading**: assemblies and distinct parts (`StepFile::read`, `read_part`,
  display names as OpenCascade XCAF gives them); a lossless Part 21 document (`p21`, byte-exact
  round trip and confined edits); the AP242 EXPRESS schema as data with an instance validator
  (`express`); semantic PMI read into a model (`pmi::read`: datums, sizes, locations, geometric
  tolerances, threads, knurls, material, general tolerances, notes, surface texture), checked on
  NIST's CAx-IF models and cross-checked against OpenCascade XCAF.
- **PMI writer** (`pmi::write`, add, replace and remove), verified by reading back
  (`pmi::verify`) and by OpenCascade reading the written files, following the maintainer's
  decisions of 2026-10-08 and 2026-10-09: material and general tolerance as specify-core writes
  them; no `mechanical_design_and_draughting_relationship` for datum feature symbols; one
  `geometric_item_specific_usage` per face; part notes as 'semantic text' and surface texture in
  AP242's surface conditions form; replace and remove of a written thread.
- **`express_rules`**: evaluated WHERE rules of the entities the writer emits (among them
  surface_texture_representation, general_property_association,
  mechanical_design_and_draughting_relationship) and the global rule
  `restrict_representation_for_surface_condition`; every written document is checked.
- **Feature recognition**: all 46 of Python quiddity's `recognise_*` entry points, each checked
  call by call against captured Python calls and over a 100-part corpus; `features::recognise`
  runs 45 families, step levels, risers and section recesses among them, as one reconciled
  inventory (Python's aggregate reconciliation); the recognition evidence view
  (`evidence_view::build_recognition_evidence`: constituent and host faces, hole-pattern members
  by position, section recesses); the section-recess family (`recognise_section_recesses`,
  `build_section_recess_document`) as library API; planar outer-profile evidence.
- **Correspondence**: revision-stable fingerprints, `correspondence::recognise` /
  `recognise_placed` (recognition in the part's own frame, reported in the file's coordinates),
  `correspond` between two revisions, and a versioned recognition document
  (`quiddity-rust/recognition/2`) for draftwright.
- **Browser builds**: haecceity runs its hidden-line work on the calling thread when no other
  thread can be spawned (wasm32-unknown-unknown), with the same results; `HAECCEITY_THREADS`
  caps the threads (e.g. 1 to time native work as a browser runs it).
- **Command line** `quiddity`: `parts`, `pmi read`, `pmi check`, `pmi write` (verified by
  reading back), `correspond` and `serve`, with versioned JSON.

### Known limitations

- Section recesses are in `Features` and correspondence but not in the recognition document
  (`quiddity <file>` lists their fingerprints without records); on corpus parts 13975 and 14052
  the family refuses where Python recovers by local degradation, and the refusal is carried, not
  published (docs/alpha1.md (b), questions 5 and 6).
- `Features` gives defining faces only and its patterns hold member copies; constituent faces
  and pattern members are in the recognition evidence view.
- The writer refuses datum targets, tolerance relations, the 'manufacturing requirement' note
  route, general tolerance tables and material density; PMI on assembly occurrences is reported,
  not modelled (`docs/step-ap242.md`).
- A thread's pitch has no place in the model and 'number of threads' is written as stated,
  pending a maintainer decision (`docs/step-ap242.md` question 6; U8).
- OpenCascade XCAF shows only the last of several part notes and no surface texture (pinned in
  `known_pmi_write.json`).
- `express_rules` evaluates only the WHERE rules named above and one of AP242's global rules
  (`GLOBAL_RULES`); it is not a general EXPRESS rule engine.
- Face triangulation refuses 2 corpus faces and the face anchor 1, each pinned in
  `crates/haecceity/tests/mesh.rs` and `anchor.rs`.
- Face moments differ from OpenCascade's adaptive integral (`BRepGProp`) by more than 1e-6 on 1358 faces
  (1345 rust-correct, 13 undetermined; `known_face_moments.json`).
- 13975's and 14052's local-degradation retry is not ported: their section-recess documents
  and recognition evidence views are refused where Python publishes (rust-wrong,
  `section_recesses/known_differences.json`).
- Every other difference from Python is listed with a verdict in the known-differences files
  the README's table names.
- CI takes 23 to 29 minutes on macOS (GitHub's 5 concurrent macOS jobs) and 11 to 16 on Linux
  (runs 38017180699 to 38031639949), against alpha 1's target of about 10 each.
