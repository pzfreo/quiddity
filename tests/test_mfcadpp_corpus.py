# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Per-face recogniser accuracy on independently authored, labelled geometry.

Every other test in this project runs on solids this project wrote. That is fine for pinning
behaviour and useless for finding out that the behaviour is wrong: a fixture built to
exercise a recogniser tends to agree with it. MFCAD++ was built by someone else, for a
different purpose, and labels *every B-Rep face* with the machining feature it belongs to —
so a record can be matched back to the face that produced it and simply checked.

That is what found the defect these tests now guard. `recognise_chamfers` was reporting the
slanted walls of steps and passages as chamfers at 44% precision, and no synthetic fixture
had noticed, because none of them contained a step whose wall looked like a chamfer.

**Attribution is by face centre**, which works because `Chamfer.at` and `AngledStep.at` are
both the recognised face's centroid rounded to three places. It is not available for families
whose records do not anchor on a face, which is why this module covers only these two.

The vendored subset and the rule that selected it are in ``corpus/mfcadpp/MANIFEST.json``.
Forty models, chosen to cover the two families plus the three classes that were being
mistaken for chamfers, from the *test* split only.
"""

from __future__ import annotations

import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

import pytest
from build123d import import_step

import quiddity as recognition
from quiddity import _recess_core as recess_core
from quiddity import recognise_angled_steps, recognise_chamfers, recognise_slots
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._dispositions import Outcome, ReasonCode
from quiddity._reconcile import (
    chamfers_that_are_not_angled_steps,
    reconcile_recesses,
)
from quiddity._registry import PHYSICAL_DEFINITIONS
from quiddity._run import start
from quiddity.result import _discover_all, _take_inventory
from tools import _legacy_recognition as legacy_recognition

sys.path.insert(0, str(Path(__file__).parents[1] / "tools"))

from per_face_scan import scan_part  # noqa: E402

CORPUS = Path(__file__).parent / "corpus" / "mfcadpp"
#: The sdist ships this module but not the 4.8 MB of STEP it reads, so absence must skip. A
#: packager running the sdist's suite is in a legitimate situation, not a broken one.
pytestmark = pytest.mark.skipif(
    not (CORPUS / "MANIFEST.json").is_file(),
    reason="the vendored MFCAD++ subset is excluded from the sdist",
)
#: MFCAD++ stores its per-face label as the name of each ``ADVANCED_FACE`` entity, in the
#: same order the kernel yields faces. ``test_the_label_to_face_mapping_holds`` is what makes
#: relying on that legitimate rather than hopeful.
_LABEL = re.compile(rb"ADVANCED_FACE\('(\d+)'")
CHAMFER, TRIANGULAR_BLIND_STEP, STOCK = 0, 20, 24


def _bevels(part):
    """The two bevel families against one ledger: what each proposed, and what survives.

    ``recognise_chamfers`` proposes a blind step's slant, because on the face alone it is a
    bevel; the rule drops the ones ``recognise_angled_steps`` claimed. Every test below that
    asks about *chamfers* asks about ``kept``, since that is what a consumer running the
    aggregate or the census receives.
    """

    ledger = ClaimLedger(FaceGraph(part))
    proposed = recognise_chamfers(part, ledger=ledger)
    steps = recognise_angled_steps(part, ledger=ledger)
    return (
        proposed,
        chamfers_that_are_not_angled_steps(proposed, steps, ledger.snapshot_index()),
        steps,
    )


def test_slot_walls_must_turn_consistently_into_one_void(corpus, monkeypatch):
    """A 0.19 mm wall overlap joins two labelled features only when AAG arcs are ignored.

    The two faces are parallel, opposed and overlap, so bounding boxes alone report a slot.
    They do not bound one recess: their shared boundary turns concavely from one wall and
    convexly from the other. The third historical candidate is the alternate-depth projection of
    a deep circular-end pocket and is now rejected by its smooth curved depth closure. Disabling
    that gate isolates the older AAG contract; disabling arc consistency then restores the two
    additional false candidates, including the grazing one from issue #119.
    """

    part = next(part for name, part, *_rest in corpus if name == "10063.step")
    assert recognise_slots(part) == []

    monkeypatch.setattr(recess_core, "_has_smooth_depth_closure", lambda *_args: False)
    assert len(recognise_slots(part)) == 1

    monkeypatch.setattr(recess_core, "_candidate_has_void_evidence", lambda *_args: True)
    assert len(recognise_slots(part)) == 1, "the AAG gate alone rejects both false pairs"

    monkeypatch.setattr(recess_core, "_bounds_one_void", lambda *_args: True)
    unguarded = recognise_slots(part)
    assert len(unguarded) == 3
    assert min(slot.d_hi - slot.d_lo for slot in unguarded) == pytest.approx(0.19, abs=1e-6)


def _is_oblique(face) -> bool:
    """A planar face aligned with no principal axis -- what a chamfer face is."""

    try:
        normal = face.normal_at()
    except Exception:  # noqa: BLE001 - a degenerate face has no normal to read
        return False
    return max(abs(normal.X), abs(normal.Y), abs(normal.Z)) < 0.99


@pytest.fixture(scope="module")
def corpus():
    """Each model paired with a face-centre to label map. Loaded once; import_step is slow."""

    models = []
    for path in sorted(CORPUS.glob("*.step")):
        labels = [int(value) for value in _LABEL.findall(path.read_bytes())]
        part = import_step(str(path))
        faces = list(part.faces())
        at_label = {}
        # strict=False deliberately: a length mismatch means the vendored corpus is wrong, and
        # `test_the_label_to_face_mapping_holds` is what should report that, with the model
        # name and both counts. Raising here would break every test in the module at fixture
        # setup instead, and say less.
        for face, label in zip(faces, labels, strict=False):
            centre = face.center()
            at_label[(round(centre.X, 3), round(centre.Y, 3), round(centre.Z, 3))] = label
        # Attribution is by rounded centroid, so two faces sharing a key would silently
        # overwrite one label with another and quietly corrupt every result below. Measured
        # at zero collisions across all 40 models; pinned so it stays that way.
        assert len(at_label) == min(len(faces), len(labels)), (
            f"{path.name}: faces share a rounded centroid"
        )
        models.append((path.name, part, labels, faces, at_label))
    assert models, "the vendored MFCAD++ subset is missing"
    return models


def test_bounded_residual_diagnostic_has_zero_development_corpus_noise(corpus) -> None:
    assert {
        name: product.diagnostics
        for name, part, *_ in corpus
        if (product := _take_inventory(part)).diagnostics
    } == {}


def test_the_selection_manifest_describes_what_is_actually_vendored():
    """The recorded rule and the files on disk agree, so the subset can be re-derived.

    A vendored corpus with no reproducible provenance is indistinguishable from an arbitrary
    pile of files that happened to pass. This is what makes it re-derivable.
    """

    manifest = json.loads((CORPUS / "MANIFEST.json").read_text(encoding="utf-8"))
    on_disk = sorted(path.name for path in CORPUS.glob("*.step"))
    rule = manifest["rule"]

    assert manifest["models"] == on_disk
    assert manifest["licence"] == "CC BY"
    assert rule["split"] == "test", "train/val models must never be vendored here"
    # Exact, not `in`: `rule["split"] in source.lower()` was satisfied by the word "latest",
    # so `source: "MFCAD++ train split, latest revision"` passed with `split: test` -- the
    # exact contradiction the check was added to catch.
    assert manifest["source"] == f"MFCAD++ {rule['split']} split"
    assert rule["order"] == "filename, ascending"
    assert rule["precondition"] == "STEP ADVANCED_FACE count equals part.faces() count"
    assert rule["targets"] == {
        "0": "Chamfer",
        "4": "6-sided passage",
        "9": "2-sided through step",
        "13": "Triangular pocket",
        "20": "Triangular blind step",
    }
    assert manifest["doi"] == ("https://doi.org/10.17034/d1fec5a0-8c10-4630-b02e-b92dc81df823"), (
        "the DOI is what locates this dataset upstream; a prefix match pins nothing"
    )

    # An earlier version stopped at the filename list, which meant the recorded rule could say
    # anything at all -- `per_label: 999`, an empty `by_label`, even `split: train` reworded --
    # and still pass. What is checkable offline is checked: the per-label counts, the stated
    # ordering, that the labelled sets account for every vendored file, and that each model
    # really does carry the label it is listed under. What is not checkable offline is that
    # these are the *first* N by filename in the upstream split, which needs the 1.5 GB
    # original; the rule records it so a future maintainer can re-derive it.
    labelled = set()
    for label, models in manifest["by_label"].items():
        assert models == sorted(models), f"label {label} is not in the recorded order"
        assert len(models) == rule["per_label"], f"label {label} has {len(models)} models"
        assert set(models) <= set(on_disk)
        labelled |= set(models)
        for name in models:
            tags = {int(v) for v in _LABEL.findall((CORPUS / name).read_bytes())}
            assert int(label) in tags, f"{name} is listed under {label} but does not carry it"
    assert labelled == set(on_disk), "vendored files that no target label accounts for"


def test_the_label_to_face_mapping_holds(corpus):
    """Every claim in this module rests on this one, so it is asserted rather than assumed.

    If ``ADVANCED_FACE`` order stopped matching ``part.faces()`` order, every test below would
    keep passing or failing for reasons unrelated to recognition. Checked two ways: the counts
    match, and label-0 faces are overwhelmingly oblique planes while other faces are not —
    which is a property of chamfers specifically, so it cannot hold by coincidence.
    """

    oblique = {True: [0, 0], False: [0, 0]}
    triangular = {True: [0, 0], False: [0, 0]}
    per_model_chamfers_all_oblique = []
    per_model_stock_all_axis_aligned = []
    for name, _part, labels, faces, _at in corpus:
        assert len(faces) == len(labels), f"{name}: {len(faces)} faces, {len(labels)} labels"
        for face, label in zip(faces, labels, strict=True):
            try:
                normal = face.normal_at()
            except Exception:  # noqa: BLE001 - a degenerate face has no normal to read
                continue
            is_oblique = max(abs(normal.X), abs(normal.Y), abs(normal.Z)) < 0.99
            oblique[label == CHAMFER][is_oblique] += 1
            triangular[label == TRIANGULAR_BLIND_STEP][len(face.edges()) == 3] += 1

        # Per model, because the sums above cannot see corruption confined to a subset. Every
        # face MFCAD++ labels Chamfer in a given model is an oblique plane -- true of all 40
        # today, and the property that breaks first if face order stops matching label order
        # for some models and not others.
        # Stock, because every model has it and the chamfer label covers only 14 of 40 --
        # rotating the labels of a model with no chamfer faces was undetectable without this.
        # Every face MFCAD++ labels Stock is axis-aligned, in all 40 models, so a rotation
        # moves an oblique face under that label and fails here.
        stock = [f for f, label in zip(faces, labels, strict=True) if label == STOCK]
        assert stock, f"{name}: no Stock faces; the per-model check would be vacuous"
        if any(_is_oblique(f) for f in stock):
            per_model_stock_all_axis_aligned.append(name)

        chamfers = [f for f, label in zip(faces, labels, strict=True) if label == CHAMFER]
        if chamfers and not all(_is_oblique(f) for f in chamfers):
            per_model_chamfers_all_oblique.append(name)

    chamfer_oblique = oblique[True][1] / sum(oblique[True])

    # Corpus-wide, this bites only for corpus-wide corruption: a rotate-by-one across all 40
    # models drops it to 0.208. Rotating the 19 models that yield no records left it at 0.917
    # and the whole module green -- and a kernel upgrade reordering faces for one topology is
    # exactly that partial shape. So the per-model check below is the one that matters, and
    # this is the aggregate summary.
    assert chamfer_oblique > 0.75, "faces labelled Chamfer are not mostly oblique planes"
    assert per_model_stock_all_axis_aligned == [], (
        f"models with an oblique face labelled Stock: {per_model_stock_all_axis_aligned}"
    )
    assert per_model_chamfers_all_oblique == [], (
        f"models whose Chamfer-labelled faces are not all oblique planes: "
        f"{per_model_chamfers_all_oblique}"
    )

    # And this is the genuinely second check, on a different label and a different property.
    # Two previous attempts were not: `other_oblique < 0.40` cannot fail, since 30.9% of all
    # 1335 faces are oblique and only 24 carry label 0, so no relabelling can push the rest
    # past 0.315. Nor could `chamfer_oblique > 2 * other_oblique` -- bounded that way, twice
    # `other_oblique` never reaches 0.63, so the 0.75 line above already implies it. A
    # redundancy replacing a tautology. Note the blind spot both shared is NOT closed by this
    # one either -- swapping labels 0 and 4 leaves label 20 untouched, so this test still
    # passes and only `test_chamfer_precision_does_not_regress` catches it.
    tri_step = triangular[True][1] / sum(triangular[True])
    tri_rest = triangular[False][1] / sum(triangular[False])
    assert tri_step > 4 * tri_rest, (
        f"the Triangular blind step label does not concentrate three-edged faces: "
        f"{tri_step:.3f} against {tri_rest:.3f} elsewhere"
    )


def test_every_angled_step_record_lands_on_a_labelled_triangular_blind_step(corpus):
    """100% precision, checked per face rather than fitted from counts.

    The discriminator is topological — a chamfer runs the full length of its edge, an angled
    step stops and a triangular flat closes it — so this is the test that fails if it is ever
    replaced by something tuned to a corpus.
    """

    wrong = []
    found = 0
    for name, part, _labels, _faces, at_label in corpus:
        for record in recognise_angled_steps(part):
            found += 1
            if at_label.get(record.at) != TRIANGULAR_BLIND_STEP:
                wrong.append((name, record.at, at_label.get(record.at)))

    assert found, "no angled steps recognised at all; the subset or the recogniser has moved"
    assert wrong == [], f"records on faces MFCAD++ does not label a blind step: {wrong}"


def test_no_chamfer_record_lands_on_a_labelled_angled_step(corpus):
    """The direct regression guard for the defect that motivated the family.

    Before `recognise_angled_steps` existed this failed on every model carrying one: on nine
    of ten such models the step's slant was the *only* chamfer reported, while the real
    chamfers on the same part were rejected. Breaking the reconciliation — the rule, the claims
    either family writes, or the blind-end test the step family gates on — reintroduces exactly
    that, and this is what says so.

    Asked of the reconciled list, which is what `feature_census` and
    `build_recognition_result` report. `recognise_chamfers` on its own proposes the slant; the
    next test is the one that pins that, and pins that the rule takes every one of them back.
    """

    stolen, resolved = [], 0
    for name, part, _labels, _faces, at_label in corpus:
        _, kept, _ = _bevels(part)
        for record in kept:
            label = at_label.get(record.at)
            resolved += label is not None
            if label == TRIANGULAR_BLIND_STEP:
                stolen.append((name, record.at))

    # Without this the test passes when attribution stops resolving entirely: replacing the
    # lookup key with one that can never match left it green, which would make "the direct
    # regression guard" a silent no-op. All 17 chamfer records resolve today.
    assert resolved, "no chamfer record resolved to a labelled face; attribution is broken"
    assert stolen == [], f"chamfer records on faces labelled a blind step: {stolen}"


def test_the_rule_takes_back_every_slant_the_chamfer_family_proposes(corpus):
    """What the reconciliation actually removes, on geometry this project did not author.

    Two claims at once, and neither is checkable from counts alone:

    - every record `recognise_chamfers` gains by no longer declining a triangular-ended bevel
      lands on a face MFCAD++ labels a *Triangular blind step* — so the proposals it now makes
      are exactly the slants, not some wider set;
    - the rule drops all of them, so nothing survives into the reconciled list that the labels
      call a step.

    Measured here: 8 of the 11 steps across 40 models are proposed as chamfers and all 8 are
    taken back. The other 3 never reach the rule — `recognise_chamfers` turns them away as
    spanning wedges, its own gate and nothing to do with the blind end, which is why "one
    proposal per step" is not the assertion.
    """

    dropped = matched = steps_found = 0
    wrong = []
    for name, part, _labels, _faces, at_label in corpus:
        proposed, kept, steps = _bevels(part)
        steps_found += len(steps)
        taken = [record for record in proposed if record not in kept]
        dropped += len(taken)
        assert len(kept) + len(taken) == len(proposed), f"{name}: the rule invented a record"
        for record in taken:
            if at_label.get(record.at) == TRIANGULAR_BLIND_STEP:
                matched += 1
            else:
                wrong.append((name, record.at, at_label.get(record.at)))

    assert steps_found == 11, f"the corpus no longer carries 11 angled steps: {steps_found}"
    assert dropped == 8, f"the rule dropped {dropped} chamfer proposals, not 8"
    assert wrong == [], f"proposals dropped on faces MFCAD++ does not label a blind step: {wrong}"
    assert matched == dropped


#: Record counts as of vendoring, **per model**. Not a correctness baseline -- a change
#: detector, as ``_OBSERVED`` is on the NIST side. Per model rather than per corpus because
#: two grand totals cannot see a redistribution: moving one angled step from the model that
#: owns it to a model that owns none leaves both sums intact, and that is a real behaviour
#: change on two of forty models. The NIST sibling was already per model; this was not, which
#: is the same asymmetry as the volume blindness it replaced.
_OBSERVED_RECORDS = {
    "10000.step": {"angled_steps": 1},
    "10007.step": {"angled_steps": 1},
    "10020.step": {"chamfers": 1},
    "10033.step": {"chamfers": 2},
    "10049.step": {"angled_steps": 1},
    "10063.step": {"angled_steps": 1},
    "10077.step": {"chamfers": 3},
    "1008.step": {"chamfers": 1},
    "10092.step": {"chamfers": 1},
    "10101.step": {"angled_steps": 1},
    "10103.step": {"chamfers": 1},
    "10119.step": {"angled_steps": 1},
    "1013.step": {"chamfers": 1},
    "10131.step": {"chamfers": 1},
    "10138.step": {"angled_steps": 2},
    "10146.step": {"chamfers": 2},
    "10163.step": {"chamfers": 1},
    "1017.step": {"chamfers": 2},
    "10170.step": {"chamfers": 1},
    "10224.step": {"angled_steps": 1},
    "10245.step": {"angled_steps": 1},
    "10247.step": {"angled_steps": 1},
}


def test_the_records_each_model_yields_have_not_moved(corpus):
    """Volume and distribution. Precision and the >=1 gates are blind to both."""

    actual = {}
    for name, part, _labels, _faces, _at in corpus:
        _, kept, steps = _bevels(part)
        counts = {"angled_steps": len(steps), "chamfers": len(kept)}
        found = {k: v for k, v in counts.items() if v}
        if found:
            actual[name] = found

    assert actual == _OBSERVED_RECORDS


def test_chamfer_precision_does_not_regress(corpus):
    """A floor, not a target: this subset is deliberately stocked with confusable classes.

    Twelve models were selected for each of triangular pocket, 2-sided through step and
    6-sided passage precisely because their walls are oblique planes that a chamfer
    recogniser can mistake, so precision here is a harder number than on a random sample —
    79% against 78% over 120 unselected models. It was 44% before the angled-step family.
    """

    records = correct = 0
    for _name, part, _labels, _faces, at_label in corpus:
        _, kept, _ = _bevels(part)
        for record in kept:
            records += 1
            correct += at_label.get(record.at) == CHAMFER

    assert records, "no chamfers recognised at all"
    assert correct / records >= 0.70, (
        f"chamfer precision fell to {100 * correct / records:.0f}% "
        f"({correct}/{records}); it was 79% when this subset was vendored and 44% before "
        "recognise_angled_steps existed"
    )


def _per_face(corpus):
    """Every claimed face across the corpus, as ``{family: Counter(label)}``.

    Attribution by *claim* rather than by matching a record's ``at`` against a face centroid,
    which is what the two tests above have to do because their families' records happen to
    anchor on a face. A claim names its faces, so this needs no coincidence of coordinates and
    works for a family whose record anchors on nothing in particular.
    """

    totals: dict[str, Counter] = defaultdict(Counter)
    for _name, part, labels, _faces, _at in corpus:
        claimed, _records, _covered = scan_part(part, labels)
        for family, counts in claimed.items():
            totals[family].update(counts)
    return totals


def test_only_plate_and_pad_boundary_roles_land_on_stock_faces(corpus):
    """A bounded taxonomy-overlap detector, not a universal no-stock invariant.

    Plate attribution truthfully owns both material-side slab boundaries. MFCAD++ labels one of
    those boundaries ``Stock`` when no subtractive feature owns it, so the 13 Stock-labelled face
    occurrences below are expected overlap rather than claims that stock itself is a machined
    feature.

    **It is not a universal law, and an earlier version of this docstring said it was.** Over
    2,000 models four claims do land on *Stock*, and the clearest of them is not a defect:
    `recognise_passages` reports a genuine 6-sided passage on `11251.step` whose six walls
    carry *five different labels*, two of them ``Stock``. MFCAD++ assigns each face to exactly
    one feature, so where features intersect, a passage wall that is also chamfered is labelled
    *Chamfer* and one bounded by raw billet is labelled *Stock*. ``Stock`` means "assigned to
    no feature", which is weaker and corpus-specific.

    Principal-axis Pad covariance adds nine equally truthful Stock-labelled roles. Inspection of
    the complete five-face occurrences found a material-outward rectangular terminal plus four
    perimeter walls; MFCAD++ labels the residual island's terminal/exterior sides Stock and its
    other sides by the intersecting subtractive features. That is the same single-label overlap,
    not permission for an incomplete face projection. All other families remain the negative
    control. A change is a prompt to inspect exact roles, not a reason to fit production
    recognition to this corpus's single-label assignment.
    """

    claimed = _per_face(corpus)
    on_stock = {family: counts[STOCK] for family, counts in claimed.items() if counts.get(STOCK)}
    assert on_stock == {"Plate": 13, "RaisedPad": 9}, (
        f"unexpected claims on stock-labelled faces: {on_stock}"
    )


def test_10060_legacy_false_positive_is_omitted_with_only_the_named_census_narrowing(
    corpus,
) -> None:
    """The rich authority keeps only the truthful member of a mixed legacy roster."""

    part = next(part for name, part, *_rest in corpus if name == "10060.step")
    legacy = legacy_recognition.recognise_passages(part)
    assert [(record.axis, record.length) for record in legacy] == [
        ("x", 11.886),
        ("z", 33.245),
    ]

    product = _take_inventory(part)
    passages = product.physical.candidate_set(FamilyId.PASSAGES).candidates
    assert len(passages) == 1
    assert product._legacy_result.section_passages == (passages[0].record,)
    assert product._legacy_result.passages == (legacy[1],)
    # Three-decimal endpoint serialization cannot encode this odd-quantum span's historical
    # midpoint: the issuer-frozen full-precision compatibility fact, not record rematching, owns
    # the exact legacy value.
    assert passages[0].record.run_interval == (0.0, 33.245)
    assert 0.5 * sum(passages[0].record.run_interval) == 16.6225
    assert legacy[1].at[2] == 16.623
    assert product.evidence.defining_of(passages[0])
    (passage_disposition,) = product.reconciliation.for_family(FamilyId.PASSAGES)
    assert passage_disposition.candidate is passages[0]
    assert (passage_disposition.outcome, passage_disposition.reason) == (
        Outcome.ACCEPTED,
        ReasonCode.DEFAULT_ACCEPTED,
    )

    # The historical alternate-depth Slot interpretation is now refused during discovery: the
    # same smooth curved region closes its alleged through axis. It no longer needs a later Pocket
    # precedence disposition.
    assert product.reconciliation.for_family(FamilyId.SLOTS) == ()
    assert [
        (item.outcome, item.reason) for item in product.reconciliation.for_family(FamilyId.POCKETS)
    ] == [
        (Outcome.ACCEPTED, ReasonCode.DEFAULT_ACCEPTED),
        (Outcome.ACCEPTED, ReasonCode.DEFAULT_ACCEPTED),
    ]
    assert product.physical.candidate_set(FamilyId.PRISMATIC_POCKETS).candidates == ()
    assert product.reconciliation.for_family(FamilyId.PRISMATIC_POCKETS) == ()
    assert product.result.slots == ()
    assert len(product._legacy_result.pockets) == 2
    assert product._legacy_result.prismatic_pockets == ()

    context = start(part)
    ledger = ClaimLedger(context.graph, definitions=PHYSICAL_DEFINITIONS)
    _discover_all(context, ledger)
    (completed,) = ledger._issuer._completed_occurrences[FamilyId.PASSAGES]
    (issued,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    assert ledger.snapshot_index().passage_compatibility(issued).legacy_ordinal == 1
    assert completed.record(type(issued.record)) is issued.record
    assert frozenset(completed.defining()) == ledger.defining_of(issued)
    assert completed.solid() is context.graph.common_valid_solid(completed.defining())
    terminal = ledger.snapshot_index()
    assert all(item.family is not FamilyId.PASSAGES for item in terminal._observations)

    public_census = recognition.feature_census(part)
    assert public_census["section_recess"] == len(product.result.section_recesses)
    assert "passage" not in public_census
    census = legacy_recognition.feature_census(part)
    assert census["passage"] == 1
    assert census == {
        "hole": 1,
        "hole_pattern": 0,
        "boss": 0,
        "step": 0,
        "groove": 0,
        "flat": 0,
        "slot": 0,
        "oriented_slot": 0,
        "rectangular_blind_slot": 0,
        "round_bottom_blind_slot": 0,
        "channel": 0,
        "pocket": 2,
        "prismatic_pocket": 0,
        "edge_open_circular_pocket": 0,
        "edge_open_prismatic_recess": 0,
        "passage": 1,
        "chamfer": 0,
        "angled_step": 0,
        # The second independently proved pair has a subdivided planar terminal; #364 makes
        # that B-Rep presentation variant part of the same physical contract.
        "paired_ramp_step": 2,
        "through_step": 0,
        "circular_blind_step": 0,
        "blend": 0,
        "fillet": 0,
        "countersink": 0,
        "plate": 1,
    }


def test_plate_stock_overlap_is_exact_low_high_boundary_evidence(corpus):
    expected = {
        "1000.step": 1,
        "10007.step": 1,
        "10033.step": 1,
        "10038.step": 2,
        "10047.step": 1,
        "10060.step": 1,
        "1007.step": 1,
        "10096.step": 1,
        "10119.step": 1,
        "10155.step": 1,
        "1017.step": 2,
    }
    observed = {}
    for name, part, labels, faces, _at in corpus:
        product = _take_inventory(part)
        graph = product.context.graph
        face_nodes = [graph.require_node(face) for face in faces]
        count = 0
        for candidate in product.accepted.candidate_set(FamilyId.PLATES).candidates:
            defining = product.evidence.defining_of(candidate)
            labelled = [
                (node, labels[index]) for index, node in enumerate(face_nodes) if node in defining
            ]
            stock_nodes = [node for node, label in labelled if label == STOCK]
            if not stock_nodes:
                continue
            count += len(stock_nodes)
            assert any(label != STOCK for _node, label in labelled)
            axis = "xyz".index(candidate.record.axis)
            signed_labels = []
            for node, label in labelled:
                normal = graph.normal(node)
                assert normal is not None
                component = normal[axis]
                assert abs(component) >= 0.99
                signed_labels.append((component > 0, label))
            assert {sign for sign, _label in signed_labels} == {False, True}
            stock_signs = {sign for sign, label in signed_labels if label == STOCK}
            assert len(stock_signs) == 1
            stock_sign = next(iter(stock_signs))
            assert all(label != STOCK for sign, label in signed_labels if sign != stock_sign)
            assert any(label != STOCK for sign, label in signed_labels if sign != stock_sign)
        if count:
            observed[name] = count
    assert observed == expected


def test_what_the_claiming_families_actually_claim_matches_the_reviewed_f4b_baseline(corpus):
    """Per-face attribution for selected families MFCAD++ can see, as a change detector.

    Not a correctness baseline. Two of these are *invariants* and the other two are
    *observations*, and the difference matters when one of them moves:

    - **Angled steps and passages are exact**, and should stay exact. Every face either claims
      is labelled the feature it says it is -- for passages across all three shape variants,
      which the family deliberately does not distinguish.
    - **Chamfer at 14 of 17** is the same 82% ``test_chamfer_precision_does_not_regress``
      measures from record centroids, arrived at independently through the ledger. The two
      disagreeing would mean one of the attribution methods is wrong.
    - **Slot is the accepted aggregate inventory.** Boundary reconciliation has removed the
      paired-wall fragments that complete pocket and passage rings explain; the remainder is
      pinned here as a change detector, not asserted to match MFCAD++'s single-label taxonomy.
    """

    claimed = _per_face(corpus)
    passages = {"Triangular passage": 2, "Rectangular passage": 3, "6-sided passage": 4}

    steps = claimed["AngledStep"]
    assert set(steps) == {TRIANGULAR_BLIND_STEP} and sum(steps.values()) == 11

    ring = claimed["SectionPassage"]
    assert set(ring) == set(passages.values()), "a passage claimed a non-passage face"
    # The complete-cycle path plus the bounded two-mouth fallback retain 170 truthfully labelled
    # walls. Edge-incidence section ordering recovered the preceding 143-face baseline; exact
    # planar termination equations now recover another 18 six-sided and 9 triangular walls whose
    # exterior stock faces are nonparallel. The old 115-face compatibility baseline included
    # partial-span rings such as 10060's X occurrence; those remain visible only from the frozen
    # writer-free legacy API and cannot own evidence.
    assert ring == Counter({4: 95, 3: 40, 2: 35})

    bevels = claimed["Chamfer"]
    assert bevels[CHAMFER] == 14 and sum(bevels.values()) == 17

    slots = claimed["Slot"]
    # Exact prism and curved-depth closure evidence remove material-crossing and alternate-depth
    # pocket interpretations. The remaining accepted walls in this frozen sample are all labelled
    # rectangular through Slot; this is a change detector, not label authority.
    assert slots == Counter({6: 4})

    rectangular_blind_slots = claimed["RectangularBlindSlot"]
    assert rectangular_blind_slots == Counter({17: 4})

    # Pockets are the blind counterpart and land mostly where the name says. Complete ring
    # containment removes the old passage fragments; partial intersections deliberately remain.
    pockets = claimed["Pocket"]
    # #236 publishes the complete route-selected role set rather than the earlier planar-only
    # compatibility projection. Distinct nested records may therefore share one original wall;
    # per-face totals count every Candidate/face attribution occurrence, not unique topology.
    # Principal-axis corner interruptions now choose their uniquely shallowest leg as depth
    # instead of treating world Z as manufacturing intent. Opposed-wall pockets also evaluate
    # both physical depth interpretations instead of letting a rejected first XYZ interpretation
    # hide a valid second one. MFCAD++'s single face label cannot make an axis choice authoritative.
    # The explicit rectangular-blind-slot precedence removes two old Pocket face claims in this
    # sample; the new family above owns the complete four-face occurrence instead.
    assert sum(pockets.values()) == 124
    assert pockets[16] == 54, "most Pocket evidence is labelled Circular end pocket"
    assert pockets[14] == 42, "rectangular ownership remains the next largest population"
    assert pockets[22] == 18, "corner depth no longer follows the dataset's world-Z presentation"
    assert pockets[3] == 2, "complete passage rings remove the old pocket fragments"


def test_accepted_recess_claims_have_no_containment_conflicts(corpus):
    """#112 leaves only compatible partial overlaps, not duplicate descriptions of one void."""

    overlaps = []
    for name, part, _labels, _faces, _at in corpus:
        graph = FaceGraph(part)
        ledger = ClaimLedger(graph)
        slots = recognition.recognise_slots(part, ledger=ledger)
        pockets = legacy_recognition.recognise_pockets(part, ledger=ledger)
        prismatic = legacy_recognition.recognise_prismatic_pockets(part, ledger=ledger)
        passages = legacy_recognition.recognise_section_passages(part, ledger=ledger)
        for family, records in (
            (FamilyId.SLOTS, slots),
            (FamilyId.POCKETS, pockets),
            (FamilyId.PRISMATIC_POCKETS, prismatic),
            (FamilyId.PASSAGES, passages),
        ):
            ledger.candidate_set_for(family, records)
        accepted = reconcile_recesses(
            slots,
            pockets,
            prismatic,
            passages,
            ledger.snapshot_index(),
        )
        records = [record for family in accepted for record in family]
        for index, left in enumerate(records):
            left_faces = ledger.defining_of(left)
            for right in records[index + 1 :]:
                if type(left) is type(right):
                    continue
                right_faces = ledger.defining_of(right)
                shared = left_faces & right_faces
                if shared:
                    overlaps.append(
                        (name, type(left).__name__, type(right).__name__, left_faces, right_faces)
                    )

    signatures = [
        (name, {left, right}, len(left_faces & right_faces))
        for name, left, right, left_faces, right_faces in overlaps
    ]
    assert signatures == [
        ("10190.step", {"Pocket", "Slot"}, 1),
    ]
    assert all(
        not left_faces <= right_faces and not right_faces <= left_faces
        for _name, _left, _right, left_faces, right_faces in overlaps
    )
