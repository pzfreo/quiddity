# K3: Python's local-degradation retry (2026-10-15)

`features::take_inventory` ports `_take_inventory`'s bounded fallback: a strict run, or, where
its hole evidence refuses for want of one valid solid on a part whose solids are not all valid
(`holes::strict_refusal_retries`, the condition `tests/internal/local_degradation.rs` already used), a
second run from scratch in which `evidence::common_valid_solid` answers as Python's degraded graph
(`locally_valid_solid`) and holes, pockets, fillets, plates, step levels, risers, rectangular
pads, slots, channels, prismatic pockets and edge-open prismatic and circular recesses skip what
Python's skip. `recognise`, `inventory`, `section_recess_projection` and
`build_recognition_evidence` go through it. 13975 and 14052 now publish Python's section recesses
and match Python's inventory and evidence view; no other corpus part's output changes.

MFInstSeg sample re-run (`examples/coverage_sweep`, every fifth model of the sorted `steps/`
listing, 12,499 models, base `0d601d2` against this branch):

| | base | K3 |
| --- | ---: | ---: |
| section-recess refusals | 47 (0.38%) | 8 (0.06%) |
| recognition completed / panicked / unreadable | 12,496 / 2 / 1 | 12,496 / 2 / 1 |
| section recesses | 28,536 | 28,583 |

68 models change: the 39 that no longer refuse and 29 others that take the retry. Over them:
holes −62, risers −43, step levels −26, fillets −22, plates −19, pockets −14, prismatic pockets
−1, section passages +10, circular blind steps +5, blends +3, freeform surfaces +2, pads +1,
edge-open prismatic recesses +1 (the gains are the degraded solid admitted where it was not).
These are not compared with Python (no Python capture of MFInstSeg). The 8 still refusing have
no hole, so neither Python's aggregate nor the port retries; Python's strict run would refuse
them too (inferred from its code, not run). The two panics are the Python-shared ones the
4583c74 sweep lists. On a 32-file corpus subset, `build_recognition_evidence` plus `recognise`
retire 305.3e9 instructions against base's 306.1e9 (three runs each, −0.24%; the view no
longer discovers holes and two pocket families a second time).

Overturnable judgement calls:

- The run's mode is a thread-local flag (`evidence::locally_degraded`), not a parameter, because
  Python's flag is a property of the whole run's graph and `common_valid_solid(part, faces)` is
  called from some fifty modules with only the part; threading a flag through all of them was a
  far larger change to shared code. The flag is set only for the retried run and restored on exit
  (also on panic); recognition spawns no threads.
- Explicit skips were ported for the families whose skips the corpus capture shows (holes,
  pockets, fillets, plates, step levels, risers) and, beyond the corpus, for those the MFInstSeg
  sample showed mattering: rectangular pads (whose strict refusal panicked a retried run,
  20221123_142528_202) and the section-recess source families (slots, channels, prismatic
  pockets, edge-open prismatic and circular recesses), whose unproved records made the
  projection refuse. Each is Python's filter on the faces Python checks (defining and consulted
  faces; for prismatic pockets Python checks the constituent faces, which the port's walls and
  consulted faces together are). Python's remaining degraded skips (through steps, angled steps,
  countersinks, bosses, grooves and others) are not ported: in a retried run those families
  publish unproved records, as the strict run does.
- The retry fires only on the strict hole refusal, as Python's aggregate does; the port proves no
  other family strictly, so Python's other strict refusals (which raise without retrying) are not
  modelled. Python's recognition *document* retries on any refusal with an invalid solid
  (`document.py`); the port's document comes from `recognise` and keeps the aggregate's narrower
  condition. The [trust contract](recognition-trust.md) now records this completed run as
  `run_mode: "locally-degraded"`; its per-record status still requires strict solid validity.
- `build_recognition_evidence` now reads the holes' constituent faces from the inventory
  (`Inventory::constituent`) instead of discovering them again, since a degraded run skips some.

# G1: assemblies slice 1, haecceity (2026-10-15)

`PartDefinition.product_id` (decoded `PRODUCT.id`, D1: reported beside the index); the editor's
assembly fixture read part by part; one part of it written alone and read by OpenCascade; one
nested sub-assembly fixture (D2: flattened to leaf parts). U1 itself was already met
(`read_part`, `StepFile::part`); these tests pin it on the files downstream uses.

Overturnable judgement calls:

- The editor's `scripts/fixtures/assembly.step` is not byte-identical to haecceity's
  `ap242/assembly/assembly.step` (header time, a plate direction, NAUO ids), so it is copied as
  `ap242/assembly/editor_assembly.step` beside it rather than replacing it.
- The single-part write uses specify-core's committed intent for the pin
  (`specify/assembly_plate_pin.intent.json`, binding part 1), so the PMI is realistic rather than
  made up. It is a new `written_cases` entry (`editor_assembly_pin_alone`), so the existing
  current-output and OpenCascade oracles cover it too; no new `references/pmi_write.json` pin was
  needed. Its capture was made for that file alone (the other captures are unchanged).
- "Part 0's instances byte-identical" is checked as everything part 0's product definition,
  product_definition_shape and faces reach, and also every original instance of the file.
- `product_id` is `PRODUCT.id` even when the display name comes from a wrapper assembly
  (wrapped.step: name 'Gehäuse Ø', id 'plate').

# G7a: section recess publication (2026-10-11)

The maintainer assigned publication of `SectionRecess` records to quiddity-rust. The existing
projection already discovers native recesses, projects uncovered prismatic pockets, passages,
edge-open prismatic/circular recesses, rectangular/round-bottom blind slots, and finally proved
pockets/channels, in Python's order. It keeps the first occurrence of each body-local
constituent-face region. Publication now uses that same run and its existing frame mapping.

Overturnable judgement calls:

- Keep `quiddity-rust/recognition/2` and the package version: this is additive, follows the
  step-level/riser precedent, and preserves the requested old document bytes. A strict family
  allowlist reader must explicitly admit `section_recesses`, `SectionRecess` and `SectionRecessRefusal` before moving its tag pin
  (G7b). This version decision preserves the acceptance requirement's existing document bytes;
  the strict reader's family allowlist still makes the addition a coordinated reader change.
- Retain all existing source families beside the new family. The review correctly overturned
  the refusal omission: occurrences and source refusals both belong under `section_recesses`.
  Derived patterns remain in the dedicated section-recess API. The carried projection refusal on
  13975/14052 and the missing local-degradation retry are unchanged.
- Use the committed Python capture at `6d544e060cd0eeb4442d2196e5df757b5a15ea55`, which matches
  the read-only oracle's HEAD. Its 15 NIST channel omissions have rust-correct verdicts backed
  by independent classifier/probe evidence; publishing them would restore Python's known bug.
  The document parity test requires every relevant verdict to remain exercised.

`tests/fixtures/section_recess_publication_baseline.json` lists the exact 30 files from
draftwright-rust's `tools/quality_parts.txt`. Its SHA256s are of compact serde bytes at
quiddity-rust `4583c740f74bf872a0214ef104feb48a6fed01a2`, before changing publication: raw
`Features`, caller-space document, and default framed document. Removing precisely the new
family must recover those bytes. No existing parity fixtures or verdicts are relaxed.

Before (caller-space recognition on the 30 quality parts):

| Metric | Records | Parts |
|---|---:|---:|
| Internal pockets | 88 | 19 |
| Internal channels | 2 | 2 |
| Projected section recesses in memory | 92 | 21 |
| Published section recesses | 0 | 0 |

After (the same source records, published through the existing unified projection):

| Published classification | Records | Parts |
|---|---:|---:|
| Pocket | 40 | 19 |
| Channel | 33 | 13 |
| Passage | 14 | 9 |
| Edge-open recess | 5 | 4 |
| Total section recesses | 92 | 21 |

Python publishes 107 occurrences: 40 pockets, 48 channels, 14 passages and 5 edge-open recesses.
The 15 extra channels are the existing rust-correct NIST verdicts. All 92 remaining occurrences
match, including fields, geometry frames, evidence, dense indices and order. All 30 raw
`Features` snapshots and 60 document snapshots (caller and inferred frames) preserve their
previous bytes after removing only the addition.

The three new tests each failed on unchanged production code (0 passed, 3 failed):
`quality_part_section_recesses_match_python` (missing records),
`inferred_frame_section_recesses_are_published` (missing framed records), and
`publication_preserves_existing_document_bytes` (missing new key). They then passed (3 passed,
0 failed). The existing four-slice `recognition_carries_section_recesses` test now also checks
every published record's fields, defining faces and frame labels on all 100 corpus parts.

Publication exposed 69 previously untested caller-space versus inferred-frame section-recess
comparisons in `tests/internal/recognition.rs`. No tolerances or comparisons were relaxed. The added
`references/framed_document.json` verdicts are 66 equivalent section-basis/order representations and
3 undetermined placement-dependent proofs. An independent reconstruction compared the section
boundary vertices, arc midpoints and low/high end-surface samples of all 222 matched records on
these 69 files in file coordinates: maximum coordinate difference 0.001557470029111883 mm, below
the existing 0.01 mm grid bound; body, classification and both evidence lists agree.

The read-only Python oracle was also run directly on `10000` and `10047` (run/end reversal and
order changes), and on all three unmatched-region parts (`10060`, `10103`, `10119`). It
reproduces all three: on 10060, caller space issues a channel on [11, 13] and the inferred frame
an edge-open obround on [11, 12, 13, 14]; on 10103 the inferred frame issues the edge-open
obround on [24, 25, 26, 27] while caller space refuses source [24, 26]; on 10119 it issues the
obround pocket on [15, 16, 17, 18] while caller space refuses source [15, 18]. The first two
inherit the existing source-family undetermined verdicts; the third is likewise undetermined.
These are pre-existing recognition/projection behaviors, now visible in the document. Which
placement-dependent proof is right remains an overturnable judgement call; this stream does
not change those recognisers.

Final required checks passed on this worktree, with `CARGO_BUILD_JOBS=8`,
`RUST_TEST_THREADS=8` and `QUIDDITY_CORPUS=/Users/paul/repos/quiddity/tests/corpus` (one cargo
command at a time):

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --release -- -D warnings`: passed.
- `QUIDDITY_CORPUS_REQUIRED=1 cargo test --workspace --release`: passed, including haecceity;
  **481 passed, 0 failed, 3 ignored**, summed over 60 result lines. The ignores are the existing
  development aids (`export_written_files`, `dump`, `face_moment_evidence`). All 31 recognition
  document tests, the 3 new publication tests and all 9 existing section-recess family tests
  passed. `git diff --check` is clean.

Files changed: `src/features/mod.rs`, `src/recognition.rs`,
`src/correspondence/fingerprint.rs`, `tests/internal/recognition.rs`,
`tests/internal/section_recess_publication.rs`, `tests/fixtures/section_recess_publication_baseline.json`,
`tests/fixtures/references/framed_document.json`, `README.md`, `CHANGELOG.md`, `docs/alpha1.md` and
this file. No commits or pushes. No Python or draftwright repository changes.

## G7a fix round (2026-10-11)

`Features::section_recess_refusals` now retains the same projection's source refusals.
`recognition::families` appends them after occurrences under `section_recesses`, and the
document publishes their Python class and defining faces. Refusals carry no geometry and
receive no correspondence fingerprint or local geometry labels; their body and face indices
are unchanged by the inferred-frame mapping. The whole-projection error on 13975/14052 is
distinct and still carried provisionally, as before. The quality-part parity test now compares
all 12 refusals on 5 parts against Python's capture, in order, as well as all occurrences.

That reconstruction (`tests/support/recess_samples.rs`, run by the caller-space document
test) was retired with that test on 2026-10-26: the framed document snapshot pins the output.
The three undetermined proof verdicts remain open.

Overturnable judgement call: retain schema v2 for this fix, preserving all previous document
bytes apart from the new family as required. G7b must admit both classes before draftwright
updates its pin; a future incompatible schema revision should be coordinated there.

Refusal publication exposed three verdict-count changes: 10103 and 10119 now include their
caller-only source refusal for the already documented placement-dependent proof (+1 each).
On MFCAD++ model 163, the same refused constituent region [10, 11, 12, 13, 32] has caller defining
faces [11, 12] and framed defining faces [10, 11, 12, 13] (+2 unmatched records). A direct
read-only Python run at 6d544e0 reproduces both evidence associations and the same refusal
reason. Overturnable judgement call: retain its equivalent verdict because the refused region
and warning are identical; the committed check pins both alternative source associations.
No comparison tolerance changed; bounds increased only by these newly published records.

Fix-round final validation: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --release -- -D warnings`, and
`QUIDDITY_CORPUS_REQUIRED=1 cargo test --workspace --release` all passed with
`CARGO_BUILD_JOBS=8`, `RUST_TEST_THREADS=8` and the corpus path above, one Cargo command at a
time. Workspace totals: **481 passed, 0 failed, 3 existing ignored tests**, across 60 result
lines, including haecceity. All 31 recognition tests and all 3 publication tests passed.
`git diff --check` is clean. No commits or pushes; the pre-existing staged changes were left
untouched. No Python or draftwright repository files changed.

## 0.1.0-alpha.2 release (2026-10-11)

Released from G7a rebased onto PRs #1 and #2. Overturnable judgement calls:

- The CHANGELOG entry adds a "Fixed" line for PR #1's mass kink-budget fix, which ships in this
  release but had no entry.
- `publication_preserves_existing_document_bytes` now sets `package.version` to the baseline's
  `0.1.0-alpha.1` before hashing: the document carries `CARGO_PKG_VERSION`, so the release bump
  would otherwise change the old-bytes hashes. Every other byte is still compared.

## 0.1.0-alpha.3 release (2026-10-14)

Released from `main` at a6add55 (PRs #3 to #8 since alpha.2), for a consistent stack with
specify-core-rust 0.1.0-alpha.2 and draftwright-rust 0.1.0-alpha.2. Overturnable judgement call:
none of PRs #3 to #8 had a CHANGELOG entry, so the release commit writes one from their
descriptions.

## 0.1.0-alpha.4 release (2026-10-16)

Released from `main` at 1decc37 (PRs #10 to #12 since alpha.3), for a consistent stack with
specify-core-rust 0.1.0-alpha.3 and draftwright-rust 0.1.0-alpha.3, which pin this release's
merge commit by rev. The Unreleased CHANGELOG entries already covered all three PRs, so the
release commit only dates them.

## 0.1.0-alpha.5 release (2026-10-17)

Released from `main` at 0fdf5dcb (PR #14 since alpha.4): recognition document 3 publishes
section recess occurrences, unified patterns and refusals for downstream readers. PR #14's
CHANGELOG entry had been placed under alpha.4; the release commit moves it into the dated
alpha.5 section, leaving alpha.4's original entries intact.

## 0.1.0-alpha.6 release (2026-10-18)

Released from `main` at f14e08f8 (PR #16 since alpha.5): bounded prism emptiness probes, a
performance change with unchanged results. PR #16 had no CHANGELOG entry; the release commit
adds it. draftwright-rust 0.1.0-alpha.5 pins this release.

## 0.1.0-alpha.7 release (2026-10-18)

Released from `main` at 6ab5f29d (PR #18 since alpha.6): derived shape aspects in semantic PMI
resolve to axes and planes (PMI document format 2, reader `haecceity-pmi-read/7`).
draftwright-rust 0.1.0-alpha.6 pins this release.
