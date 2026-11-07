# Strategy: AP242 PMI as a shared crate, with a solidcore bridge (draft, 2026-11-06)

Status: **proposal**. Nothing here has been started or agreed with solidcore. Background on the
kernels compared is in [kernel-comparison-2026-11.md](kernel-comparison-2026-11.md).

## 1. The decision in one paragraph

Our strategic value is in quiddity (feature recognition) and Specify (PMI from design intent), not
in owning a general CAD kernel. Of haecceity's code, roughly 15k lines are general geometry, a
commodity that larger projects (solidcore, nacre) are investing in. Roughly 45k lines are STEP,
EXPRESS rules and **AP242 PMI**, which no other Rust project has. So:

- **Keep haecceity** as quiddity's reader and geometry query engine, kept narrow, with no
  modelling operations.
- **Extract the PMI layer into its own crate, `ap242`**, that does not depend on any kernel or any
  STEP parser.
- **Publish a bridge, `ap242-solidcore`**, so solidcore users can read STEP with its PMI, keep the
  PMI correct through solidcore's modelling operations, and write AP242 back out. solidcore does
  not have to take any dependency on us. This is "shape C".
- Later, if solidcore wants it, it can depend on `ap242` directly (shape B). The design below keeps
  that door open without requiring it.

## 2. Facts this rests on (checked 2026-11-06)

| Fact | Where |
|---|---|
| solidcore's owners decided "Importers are solidcore's. There is one STEP implementation", enforced as one implementation per job in CI. | solidcore `docs/layers.md` §2, §4 |
| solidcore's STEP crates (`step-import`, `step-brep`, `step-export`) are `publish = false`. The published `solidcore` crate has **no STEP import**. | their `Cargo.toml` files |
| solidcore has no PMI, GD&T or AP242 semantics anywhere. | their docs, plans and code |
| The published kernel has a public attribute store (`solidcore::geom::attribute`). The unpublished `solidcore-ops::provenance` carries application data on faces and edges through **split, merge and coincident** events during operations. | `crates/solidcore/src/geom/attribute.rs`, `crates/solidcore-ops/src/provenance.rs` |
| solidcore builds bodies from a neutral model using only the kernel's public API. | `step-brep` depends on `solidcore` and `step-import` only |
| solidcore's external dependencies are general-purpose crates only (`serde`, `thiserror`, `spade`, `miniz_oxide`, `toml`); `cargo-deny` allows permissive licences from crates.io. | `deny.toml`, crate manifests |
| `step-io` (our parser) is by nacre's author and depends on nothing kernel-related (`logos`, `memchr`). Its parser layer is about 3.5k lines. | `elgar328/step-io` |
| haecceity's PMI code calls the geometry modules only about 15 times, mostly small vector helpers. Its real couplings are to `p21`, `express`, `express_rules`, `step::PartDefinition` and `step::PartReader`, plus the B-rep for callout placement. | `crates/haecceity/src/{pmi,presentation,json}` |

## 3. Target architecture

```
                    ┌─────────────────────────────────────────────┐
                    │ ap242  (new, kernel-free, parser-free)      │
                    │  semantic PMI model · read · write · verify │
                    │  EXPRESS tables and rules · JSON interchange│
                    │  talks to files through `EntityGraph`       │
                    └──────────────┬──────────────────────────────┘
                                   │ trait EntityGraph (read) + EntityEdit (write)
                                   │ trait PlacementGeometry (callouts only)
          ┌────────────────────────┼─────────────────────────────┐
          │                        │                             │
 ┌────────▼─────────┐   ┌──────────▼──────────┐       ┌──────────▼──────────┐
 │ ap242-step-io    │   │ ap242-presentation  │       │ ap242-solidcore     │
 │ EntityGraph over │   │ callouts, lettering │       │ bridge: read STEP → │
 │ step-io's parser │   │ (needs geometry)    │       │ solidcore Brep, tag │
 └────────┬─────────┘   └──────────┬──────────┘       │ faces, carry PMI    │
          │                        │                  │ through operations, │
 ┌────────▼────────────────────────▼──┐               │ write AP242         │
 │ haecceity (geometry engine only)   │               └──────────┬──────────┘
 │ STEP geometry import, Part, rays,  │                          │
 │ classify, volumes, mesh, HLR       │                    solidcore (crates.io)
 └────────┬───────────────────────────┘
          │
 ┌────────▼───────────┐   ┌──────────────┐
 │ quiddity           │   │ Specify      │
 │ recognition, CLI,  │   │ intent → PMI │
 │ Python             │   │              │
 └────────────────────┘   └──────────────┘
```

Design rules:

1. **`ap242` contains no STEP parser.** It reads and writes through two small traits. That is what
   lets solidcore (one parser per job) depend on it later, through an adapter over their own
   parser.
2. **`ap242` contains no geometry.** Placing callouts needs face and edge geometry, so that goes in
   `ap242-presentation`, behind a `PlacementGeometry` trait that haecceity's `Part` implements and
   solidcore's `Brep` can implement too.
3. **Faces are named by STEP entity id**, the file's own `#n`. That is the one key every parser and
   kernel shares.
4. **Same bar as solidcore's dependencies:** MIT/Apache-2.0 dual licence, few dependencies,
   `serde` behind a feature, no `unwrap`, `expect`, `panic` or slice indexing in library code,
   minimum Rust version 1.85 or lower, builds for `wasm32-unknown-unknown`.

## 4. What a solidcore user gets

```rust
let mut doc = ap242_solidcore::read(&bytes)?;          // bodies + PMI, faces tagged with #ids
let body = kernel.boolean_subtract(&doc.bodies[0], &tool)?;
let report = doc.pmi.reconcile(&body);                 // kept, split → composite, orphaned
ap242_solidcore::write(&body, &doc.pmi, &mut out)?;    // AP242 geometry + PMI
```

The function names are illustrative.

- **Read.** Parse with step-io, build `Brep`s through solidcore's public construction API (the
  path `step-brep` takes), run their repair step, and attach a `StepId` attribute to each face and
  edge.
- **Model.** The attribute handlers apply *our* PMI rules: a face split in two passes its
  tolerance to a composite of both pieces; a coincident face hands its tags to the survivor; a
  deleted face orphans its PMI, which is reported, never silently dropped.
- **Write.** Write the body's geometry, then write the PMI against the new entity ids, looked up
  through the tags.

When solidcore publishes its own importer, the bridge switches to it and only adds the tags. The
ask to solidcore then shrinks to: *"have your importer attach the STEP id as an attribute."*

## 5. Changes to haecceity

| # | Change | Size | Notes |
|---|---|---|---|
| H1 | **Define `EntityGraph` and `EntityEdit`** over what `pmi/read.rs` and `pmi/write.rs` use from `p21.rs` today: `RawEntity`, `Attribute`, complex entities, `Document` and `Edit`/`NewId`/`Record`. Implement them for step-io in `p21.rs`. | M | `p21.rs` already wraps step-io and re-exports its types, so most of the seam exists. The write side (in-place edits that keep foreign entities) is the hard part to get right. |
| H2 | **Move `PartDefinition` and the product-structure reading** out of `step.rs` into something `ap242` can own (it is AP242 product data, not geometry). `PartReader`'s presentation use goes behind the trait. | M | `pmi/read.rs`, `pmi/write.rs`, `pmi/verify.rs` and `json/mod.rs` import `step::PartDefinition`; `write.rs` uses `step::PartReader`. |
| H3 | **Move into `ap242`:** `pmi/`, `express.rs`, `express_table.rs` (generated), `express_rules.rs`, `removal.rs`, `json/` and `schema/`. haecceity re-exports them so its public API doesn't break in one go. | L (mostly moves) | The generated table and `build-walker.py` go with them. Keep the `json` and `json-import` features. |
| H4 | **Split `presentation/` into its own crate** behind `PlacementGeometry`: edge midpoints, semantic edges, face anchors and bounds. Implement the trait for `Part`. | M | Couplings found: `presentation/linear.rs` (`brep::Edge`, `Part`, `step::PartDefinition`), `json/presentation.rs::generated(…, &Part)`, `pmi/sites.rs` (`uv::FaceDomain`), plus `anchor.rs`. `stroke_font.rs` moves with it. |
| H5 | **Replace the ~15 geometry helper calls** in the PMI code (`geom::unit`, `dot`, `norm`, `sub`, `scale`, `Bounds`) with a few private helpers in `ap242`. | S | |
| H6 | **Raise `ap242` to solidcore's lint bar:** deny `unwrap`, `expect`, `panic`, `unreachable` and `indexing_slicing` in library code (about 650 direct indexing sites in the PMI, presentation and JSON code). | M, mechanical | haecceity itself can stay at its current warn level. |
| H7 | **Freeze haecceity's scope:** its README and crate docs say geometry engine only. No new modelling. `sweep.rs` stays as it is until it can be replaced (see Q4). | S | |
| H8 | **Relicense the extracted crates as MIT/Apache-2.0 dual.** haecceity stays Apache-2.0. | S, needs a decision | Possible because you are the sole author. Check `NOTICE` for any third-party text carried with the PMI code (for example the Hershey font, which is public domain). |
| H9 | **Keep step-io behind `p21.rs` only, pinned exactly.** Nothing in `ap242` names step-io; the `ap242-step-io` adapter does. | S | This is what makes a later parser swap, or a fork of step-io's ~3.5k-line parser, a one-module change. |

What does **not** change in haecceity: `step.rs` geometry import and its OpenCascade parity quirks,
`brep`, `geom`, `rays`, `classify`, `volume`, `mesh`, `hlr`, `recover` and `brep_write`. The
geometry engine is untouched.

## 6. Changes to quiddity

| # | Change | Size | Notes |
|---|---|---|---|
| Q1 | **Repoint the PMI imports:** `src/pmi_json.rs`, `src/cli.rs`, `src/dimensioning/mod.rs` and `src/bin/quiddity/pmi_interchange.rs` use `kernel::pmi` / `haecceity::pmi` / `kernel::json` today. Use `ap242::…`, or keep the haecceity re-exports during the move. | S | With the re-exports from H3 this is a no-op at first, so it can be done at leisure. |
| Q2 | **Python bindings:** `crates/quiddity-py/src/pmi.rs` uses `pmi::{read, PmiRead, PartId}`. Repoint them; the Python API is unchanged. | S | The wheel stays abi3 with no Python dependencies. |
| Q3 | **Cargo features:** `json-import` and `f3d` pass through to haecceity today. `json-import` would pass through to `ap242` instead. | S | |
| Q4 | **(Optional, later) Volume probes through solidcore**, behind a `solidcore` feature, replacing `sweep.rs`'s limited extrude (straight edges and arcs about the sweep only). Results count as `unproved` until verified. | M | Only after the bridge has proved the conversion from `Part` to `Brep`. Not needed for this plan. |
| Q5 | **Golden outputs unchanged.** Recognition JSON (`recognition/6`), PMI JSON and the written AP242 files must be byte-identical before and after the extraction. | gate | This is the acceptance test for every step of H1–H6. |

What does **not** change in quiddity: recognition, correspondence, face accounting, the CLI's
output and the Python API.

## 7. New crates

| Crate | Depends on | Licence | Published |
|---|---|---|---|
| `ap242` | none at runtime (`serde` and `serde_json` optional) | MIT OR Apache-2.0 | yes |
| `ap242-step-io` | `ap242`, `step-io` (pinned) | MIT OR Apache-2.0 | yes |
| `ap242-presentation` | `ap242` | MIT OR Apache-2.0 | yes |
| `ap242-solidcore` | `ap242`, `ap242-step-io`, `ap242-presentation`, `solidcore` (pinned) | Apache-2.0 (solidcore's NOTICE carried if any code is adapted from `step-brep`) | yes, once the round trip works |

They can live in this repository as workspace members at first, to keep moves atomic, and move to
their own repository when the API settles. The names are placeholders: check crates.io for
availability and for confusion with existing crates before publishing.

## 8. Phases and gates

| Phase | Work | Gate to move on |
|---|---|---|
| **0. Prove the bridge** | A throwaway prototype outside this repo: one AP242 test file → haecceity `Part` → solidcore `Brep` with `StepId` attributes → one boolean → check the tags survived → print which PMI is kept, split or orphaned. | The tags survive a boolean on a real file. If solidcore's attribute API can't do this from the published crate, stop and ask them before going further. |
| **1. Extract `ap242` inside haecceity** | H1, H2, H3, H5, H9 as modules inside haecceity, behind the traits, with the re-exports in place. | Q5: all golden outputs identical. |
| **2. Split the crates** | Move to workspace crates `ap242`, `ap242-step-io` and `ap242-presentation` (H4). Do Q1–Q3. | Q5 again; `cargo check --target wasm32-unknown-unknown` on `ap242`. |
| **3. Harden** | H6 lint bar, H8 licence, docs and examples written for a solidcore user. | Clean clippy at solidcore's settings. |
| **4. Ship the bridge** | `ap242-solidcore`: read, reconcile and write, with documented split, merge and orphan rules. Round-trip tests on the AP242 test files. | A full round trip on the AP242 write fixtures, with OpenCascade (OCP) loading the result (the same check as `tools/check_pmi_occt.py`). |
| **5. Talk to solidcore** | Open an issue with something working: (a) the importer attaches STEP ids as attributes; (b) would they take an optional dependency on `ap242` over their own parser (shape B); (c) offer our STEP test files for their STEP merge. | Their answer. Shape C stands either way. |

## 9. Risks

| Risk | Mitigation |
|---|---|
| solidcore's API changes fast and they keep no shims (for example, a recent change made `Brep` non-`Clone`). | Pin exactly; the bridge is the only crate that touches solidcore. |
| The bridge duplicates their STEP importer, which they may see as competing. | Present it as interim. Switch to their importer once it's published, and say so up front. |
| solidcore's validity checks reject vendor files that haecceity accepts. | Run their repair step after building and report what it changed; never hide a refusal. |
| step-io is maintained by one person. | H9 keeps it to one adapter; its parser is small enough to fork. |
| Splitting crates causes churn for Specify and Draftwright. | Keep the haecceity re-exports for one minor-version cycle, with a migration note in `CHANGELOG.md`. |
| Rules for PMI after a face is split or merged are new semantic ground. | Start conservative: split → composite, merge → keep if the PMI is identical, otherwise orphan; deleted → orphan. Report everything. |
| Bus factor on our side too. | Publishing the crates on their own is the best way to attract other maintainers. |

## 10. Open questions

1. Relicensing the extracted crates as dual MIT/Apache-2.0 (H8): yes or no?
2. One repository or separate ones for `ap242*`, and when?
3. Should `ap242-presentation` be part of the solidcore offer, or stay a Specify/Draftwright
   concern?
4. Crate names: `ap242` was unclaimed on crates.io on 2026-11-06. Claim it early, or choose a less generic name?
5. Does Specify want to edit geometry (and so use the bridge itself), or only annotate it?
