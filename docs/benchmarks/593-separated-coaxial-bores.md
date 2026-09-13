# Separate coaxial bore occurrences across exterior gaps

Issue [#593](https://github.com/pzfreo/quiddity/issues/593).
Baseline source: `75212b2e02d3be04a84c5ef1b6b14b7c0c467a83` (Quiddity 0.2.8).

## Geometry and compatibility

The original same-diameter branch in `_merge_stacks` treated two non-closed ends
as sufficient evidence of one interrupted bore. The whistle-key frame has one
connected solid but six bored lugs separated by exterior air, so neither common
solid ownership nor a void centreline midpoint distinguishes it from a cross-drilling.

Recombination now requires both lands to adjoin original faces belonging to
the same eligible internal cylindrical segment in the input inventory. Its finite
analytic cylinder must also contain the bore-axis interval across the gap. Exact
source-edge adjacency avoids the limited end-classification window, which can
miss oblique intersection rims; analytic containment rejects mere lateral contact.
Axial bounds use the existing contiguous-segment tolerance, and radial comparison
allows the inventory's six-significant-figure quantisation plus coordinate noise.
This is a bounded cylindrical-source proof, not a general cavity-emptiness test.

Source membership is retained across native seam subdivision and certified
recovered surfaces. The map and segment inventory are frozen before recombination;
a newly merged span cannot become authority for a later merge. This uses existing
discovery and cached source adjacency, without another recogniser, graph, material
probe or public schema.

The independent cone/torus shoulder-transition branch, closed-end exclusions,
contiguous-range tolerances and two-decimal depth publication are unchanged.
Noncontiguous interruptions without the common cylindrical source or an existing
shoulder-transition proof remain separate stacks. This does not claim recognition
of arbitrary communicating cavities. Interruption faces provide context and do
not become defining faces of the bore they interrupt.

Consumers may observe more distinct holes with shorter depths on affected parts.
The six occurrences and their axes are geometric facts; downstream owns any
coaxiality requirement or annotation. Seat recognition (#594) is separate.

## Supplied real part

Source: `tests/fixtures/issue_1595_whistle_key_frame.step` on Draftwright branch
[`issue-1595-whistle-key-frame-fixture`](https://github.com/pzfreo/draftwright/tree/issue-1595-whistle-key-frame-fixture),
supplied by the issue author from `pzfreo/whistle-key` at `7fb4d14`.
SHA-256: `8f060dfd4eeb4ff9589243f85eea8ca027784ac51ae98dc2d29a2371a2d115d0`.
The hash identifies the exact tested bytes even if the branch later changes.

The STEP contains one solid and 222 faces. Its six Ø1.1 cylindrical bore lands
are **2.7 mm** long, not the approximately 6 mm in the original issue prose.
Both the raw and framed aggregate/evidence routes now return six Ø1.1 × 2.7
through holes, replacing the single Ø1.1 × 53.09 record. Separate diagnostic
verification confirms the three Ø14.3 seat faces remain outside this repair.

Reproduce using the supplied file, verifying its hash first:

```python
from quiddity import import_step_geometry, build_framed_recognition_evidence
from quiddity.evidence import build_recognition_evidence

part = import_step_geometry("issue_1595_whistle_key_frame.step")
for view in (build_recognition_evidence(part), build_framed_recognition_evidence(part)):
    holes = [h for h in view.result.holes if abs(h.diameter - 1.1) < 1e-6]
    assert len(holes) == 6
    assert all(h.depth == 2.7 and h.bottom == "through" for h in holes)
```

## Authored regression evidence

`tests/test_separated_coaxial_bores.py` supplies independent construction-defined
geometry rather than requiring a network download in CI. Its 45 cases cover:

- two, three and six lugs at different exterior spacings, with and without entry chamfers;
- 0.05×, 1× and 100× scale plus arbitrary rigid placement through the framed evidence route;
- STEP round-trip and six disjoint original cylindrical defining faces;
- one and two genuine cross-drillings that must preserve the complete interrupted bore;
- oblique and offset crossings at three scales, raw/framed routes and rigid placement;
- lateral cavity contact that must not join two lands across exterior air;
- a thin cross-bore near a rounded inventory-radius boundary;
- a genuine internal interruption followed by an exterior gap in the same body;
- native/recovered crossing cylinders subdivided into patches, with reversed injected inventory.

The first separated-lug case fails on baseline source (one hole instead of two).
All 45 cases pass with the repair. All 134 existing `test_recognition.py` and
`test_hole_attribution.py` cases also pass.

## Development corpus comparison

Run the existing scorer separately with baseline and repaired source imports:

```bash
python tools/run_effectiveness_baseline.py mfcadpp tests/corpus/mfcadpp \
  --dataset-version 'vendored MFCAD++ development subset' \
  --taxonomy docs/benchmarks/effectiveness-taxonomy-v13.json \
  --limit 40 --allow-invalid --workers 2 --output /tmp/bore-comparison.json
```

Both runs select and evaluate 40 models with zero invalid inputs. All per-model
scored fields, physical family counts, defining precision/recall, face coverage,
reconciliation outcomes and aggregate summary values are identical. Runtime is
excluded from equality. This is a correctness fix demonstrated by the real part
and authored adversaries, not a claimed synthetic-corpus recall improvement.

The complete 2,500-model development population was not available locally. This
40-model comparison is not a substitute for that population or an MFInstSeg
transfer measurement. No MFInstSeg geometry was inspected or scored for this fix.

The final comparison used Python 3.14.7, build123d 0.11.1 and OCP 7.9.3.1 on
macOS arm64. Both runs used installed package metadata 0.2.8. Selected-ID SHA-256:
`f5efce2111616a9c0585c2f96a58f32aa258590d77cda3bdd744618807001fdc`.
The repaired `_hole_features.py` SHA-256 is
`4626e08e9a45dfc03c2a1b3eed37727704ac2c5c6980c4050b6e80a52e5de559`.

## Broader vendored regression comparison

An additional before/after `build_recognition_document` sweep completed all
89 vendored STEP files without errors, including the historically named
`mfcadpp_holdout` directory (MFCAD++, not MFInstSeg). Excluding the installed
package-version header, 86 documents are identical. Three have changes:

| Part | Hole occurrences before → after | Changed geometry |
| --- | --- | --- |
| CADGenBench flanged spool 132 | 19 → 31 | Twelve Ø8 × 113 merged records become twenty-four Ø8 × 5 flange bores. |
| NIST CTC-02 | 83 → 85 | Opposed Ø40 × 844 and Ø52 × 514 stacks separate into paired 49 and 32 mm bore lands, respectively, with their own entry steps. |
| NIST CTC-04 | 54 → 56 | Two Ø10 × 80 records become four Ø10 × 27.5 bore lands. |

The CTC-02 entry-step correction adds six original defining faces and removes
two from Hole membership; associated faces rise from 362/664 to 366/664.
The other two parts retain the same associated-face union. Other physical
families retain their records. These observed output changes follow the bounded
interruption contract above; they are not an independently labelled accuracy score.

The spool also exposes an existing derived-pattern defect: the two separated
12-hole flanges project onto identical 2-D points, so their Ø110 bolt circles
are absent. The separate Ø90 bolt circle survives. Independently constructed
HoleRecords reproduce the pattern defect on original main without this repair.
It is tracked separately as [#595](https://github.com/pzfreo/quiddity/issues/595);
preserving its former single circle would retain false 113 mm bore occurrences.

## Independent review

Independent review identified an oblique-crossing regression in the initial repair.
The correction retains Ø6 × 60 through drilling at 45°, rejects a shared lateral
cavity as a bridge between two Ø6 × 4 lands, and preserves a Ø0.1 × 200 crossing
when its interruption radius rounds down in the inventory. All three findings are
resolved. A separate 90-case authored crossing comparison matches baseline.

A centred crossing with constructor rotation `(0, 60, 0)` exposed an existing
axis-key collision on original main: nonparallel axes sharing a dominant letter
and projected origin are combined into a false counterbore. It is tracked as
[#597](https://github.com/pzfreo/quiddity/issues/597). The positive regression cases
here exercise baseline-supported axes; this repair does not change `_line_key`.

## Checks

- Full local suite: **7,838 passed**, **95.23%** line/branch coverage; the existing
  91% coverage gate passed without adjustment.
- Repository Ruff lint and formatting checks pass; mypy passes for 95 source files.
- The initial broad run exposed stale editable metadata (0.2.6.dev0 versus the
  checked-in 0.2.8 manifest). `uv sync --dev --frozen` refreshed the installation;
  the complete successful run above used 0.2.8. No manifest policy was weakened.

The correction's composite runtime check measured 1.730 s against the fixed
1.191 s ceiling. Original main measured 1.721 s in the same loaded window,
a difference of 0.5%; both checks were over budget. The machine's one-minute
load average was 31.93, versus the below-4 condition of the checked-in baseline.
The initial census check was also over budget (correction 24.570 s; original
main 21.748 s as load fell). After the full suite finished, the correction's
census check passed at 12.709 s against 13.189 s. Composite improved to 1.397 s
but remained above 1.191 s. A passing composite runtime gate is not claimed;
all budgets remain unchanged.

Hosted platform/version checks are separate PR checks; no package release or
Draftwright dependency-pin change is included.
