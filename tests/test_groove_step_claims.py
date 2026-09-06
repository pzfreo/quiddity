# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""One band, two true records, and a metric that counts it once (#95).

A groove is an external band whose OD is a local minimum. The turned-step ladder describes the
same shaft, and that band is one of its rungs — so `recognise_grooves` and
`recognise_turned_steps` both report it, both correctly. It was counted twice.

The fix is not to drop either record, and these tests pin that: the ladder keeps its rung,
because `TurnedProfile` reads an interior end as a real end face and a ladder with a gap in it
describes a shaft with two faces where the groove is. Only `feature_census`, which counts
distinct machined features rather than describing them, should treat the band as one.

The rule waited on a defect upstream of the count — while `recognise_turned_steps`
reported the groove's rung at the shaft's OD on a part modelled small, wiring it would have
traded one wrong count for a scale-dependent one. That is fixed, so the count is pinned here
at three scales.
"""

from __future__ import annotations

import pytest
from attribution_audit import attributed_run
from build123d import Axis, Compound, Cylinder, Pos, Rotation

import quiddity as r
import quiddity.grooves as groove_module
import quiddity.result as result_module
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._dispositions import Outcome, ReasonCode
from quiddity._reconcile import steps_that_are_not_grooves


def _grooved_shaft():
    """A two-diameter shaft with an annular groove cut into the larger band."""

    shaft = Cylinder(20, 60) + Pos(0, 0, 40) * Cylinder(14, 20)
    return shaft - Pos(0, 0, 10) * (Cylinder(20, 6) - Cylinder(16, 6))


def _plain_shaft():
    """The same shaft with no groove: the contrast case for the count."""

    return Cylinder(20, 60) + Pos(0, 0, 40) * Cylinder(14, 20)


def _claimed(part):
    """Both families against one ledger, proved to return what they return without it."""

    cyls = r.analyse_cylinders(part)
    ledger, grooves = attributed_run(
        part,
        FamilyId.GROOVES,
        r.recognise_grooves,
        kwargs={"cyls": cyls},
    )
    steps = r.recognise_turned_steps(part, cyls=cyls, ledger=ledger)

    plain_steps = r.recognise_turned_steps(part, cyls=cyls)
    assert steps == plain_steps, "claiming changed what was recognised"
    assert [step.to_dict() for step in steps] == [step.to_dict() for step in plain_steps]
    candidates = ledger.candidate_set(FamilyId.TURNED_STEPS).candidates
    assert len(candidates) == len(steps)
    for candidate, step in zip(candidates, steps, strict=True):
        assert candidate.record is step
        defining = ledger.defining_of(candidate)
        assert defining
        assert ledger.graph.common_valid_solid(defining) is not None
    return ledger, grooves, steps


def test_a_groove_claims_its_floor_band_and_not_the_shaft_either_side():
    """The walls make the band a local minimum; they do not bound the groove.

    Claiming them would have every groove contest the two steps it sits between, which is the
    conflict this reconciliation exists to resolve rather than to manufacture.
    """

    part = _grooved_shaft()
    ledger, grooves, _ = _claimed(part)
    (groove,) = grooves

    claim = next(c for c in ledger.claims if c.claimant is groove)
    (node,) = claim.defining
    lo, hi = ledger.graph.bounds(node)[2]
    assert hi - lo == groove.width, "the claimed face spans exactly the groove's width"
    radius = max(abs(edge) for edge in ledger.graph.bounds(node)[0])
    assert 2 * radius == groove.diameter, "and it is the floor band, not a wall"


def test_multiple_grooves_keep_occurrence_identity_and_floor_roles() -> None:
    shaft = Cylinder(20, 80)
    for position in (10, 35):
        shaft -= Pos(0, 0, position) * (Cylinder(20, 6) - Cylinder(16, 6))
    ledger, grooves = attributed_run(
        shaft,
        FamilyId.GROOVES,
        r.recognise_grooves,
        kwargs={"cyls": r.analyse_cylinders(shaft)},
    )

    assert len(grooves) == 2
    candidates = ledger.candidate_set(FamilyId.GROOVES).candidates
    assert all(
        candidate.record is groove for candidate, groove in zip(candidates, grooves, strict=True)
    )
    assert len({next(iter(ledger.defining_of(candidate))) for candidate in candidates}) == 2


def test_a_turned_step_claims_the_bands_that_set_its_diameter():
    """The shoulder planes come from the neighbouring steps' faces, so they are not claimed."""

    part = _grooved_shaft()
    ledger, _, steps = _claimed(part)

    for step in steps:
        claim = next(c for c in ledger.claims if c.claimant is step)
        assert claim.defining, "every rung rests on a band"
        for node in claim.defining:
            radius = max(abs(edge) for edge in ledger.graph.bounds(node)[0])
            assert 2 * round(radius, 3) == step.diameter


def test_the_rule_finds_the_rung_the_groove_is():
    """What the reconciliation removes, proved on the pair rather than through the count."""

    ledger, grooves, steps = _claimed(_grooved_shaft())
    (groove,) = grooves

    assert groove.profile is not None
    assert {step.profile for step in steps} == {groove.profile}

    kept = steps_that_are_not_grooves(steps, grooves, ledger.snapshot_index())
    assert len(kept) == len(steps) - 1
    assert [step for step in steps if step not in kept][0].diameter == groove.diameter

    plain_ledger, plain_grooves, plain_steps = _claimed(_plain_shaft())
    assert plain_grooves == []
    assert (
        steps_that_are_not_grooves(plain_steps, plain_grooves, plain_ledger.snapshot_index())
        == plain_steps
    )


def test_parallel_grooved_shafts_publish_distinct_matching_profile_keys() -> None:
    left = Pos(-30, 0, 0) * _grooved_shaft()
    right = Pos(30, 0, 0) * _grooved_shaft()
    part = Compound(children=[right, left])

    grooves = r.recognise_grooves(part)
    steps = r.recognise_turned_steps(part)

    assert len(grooves) == 2
    assert all(groove.profile is not None for groove in grooves)
    assert len({groove.profile for groove in grooves}) == 2
    for groove in grooves:
        matching = [step for step in steps if step.profile == groove.profile]
        assert matching
        assert any(step.diameter == groove.diameter for step in matching)
        assert all(
            step.profile != groove.profile
            for other in grooves
            if other is not groove
            for step in steps
            if step.profile == other.profile
        )

    reversed_grooves = r.recognise_grooves(Compound(children=[left, right]))
    assert reversed_grooves == grooves


def test_all_groove_ownership_validates_before_any_candidate_is_published(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    part = Compound(children=[Pos(-30, 0, 0) * _grooved_shaft(), Pos(30, 0, 0) * _grooved_shaft()])
    ledger = ClaimLedger(FaceGraph(part))
    original = FaceGraph.common_valid_solid
    calls = 0

    def fail_later_proposal(graph: FaceGraph, nodes):
        nonlocal calls
        calls += 1
        return None if calls == 2 else original(graph, nodes)

    monkeypatch.setattr(FaceGraph, "common_valid_solid", fail_later_proposal)

    with pytest.raises(ValueError, match="no common valid solid"):
        r.recognise_grooves(part, ledger=ledger)
    assert calls == 2
    assert ledger.claims == ()


@pytest.mark.parametrize("invalid_solid_idx", (1, "not-an-index"))
def test_injected_groove_inventory_must_name_a_real_owning_solid(
    invalid_solid_idx,
) -> None:
    part = _grooved_shaft()
    z_cyls, cross_cyls = r.analyse_cylinders(part)
    invalid = [dict(cylinder, solid_idx=invalid_solid_idx) for cylinder in z_cyls]

    with pytest.raises(ValueError, match="no owning solid"):
        r.recognise_grooves(part, cyls=(invalid, cross_cyls))


def test_equal_profile_keys_cannot_claim_two_source_solids(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    left = Pos(-30, 0, 0) * _grooved_shaft()
    right = Pos(30, 0, 0) * _grooved_shaft()
    part = Compound(children=[left, right])
    ledger = ClaimLedger(FaceGraph(part))
    profile = r.recognise_grooves(left)[0].profile
    assert profile is not None
    monkeypatch.setattr(
        groove_module,
        "profile_key_from_bands",
        lambda _part, _axis, _bands, *, body_key=(): profile,
    )

    with pytest.raises(ValueError, match="profile key identifies multiple"):
        r.recognise_grooves(part, ledger=ledger)
    assert ledger.claims == ()


def test_nested_coaxial_bodies_do_not_share_the_outer_groove_membership() -> None:
    outer = _grooved_shaft() - Cylinder(6, 80)
    inner = Cylinder(5, 40) + Pos(0, 0, 40) * Cylinder(4, 40)
    part = Compound(children=[outer, inner])

    (groove,) = r.recognise_grooves(part)
    profiles = r.TurnedProfile.grouped_from_steps(r.recognise_turned_steps(part))

    assert len(profiles) == 2
    assert groove.profile is not None
    assert [profile.profile == groove.profile for profile in profiles].count(True) == 1
    owner = next(profile for profile in profiles if profile.profile == groove.profile)
    sibling = next(profile for profile in profiles if profile.profile != groove.profile)
    assert any(step.diameter == groove.diameter for step in owner.steps)
    assert all(step.profile != groove.profile for step in sibling.steps)


@pytest.mark.parametrize(
    ("rotation", "axis"),
    (
        (Rotation(0, 0, 0), "z"),
        (Rotation(0, 90, 0), "x"),
        (Rotation(90, 0, 0), "y"),
    ),
)
def test_groove_profile_membership_is_principal_axis_covariant(rotation, axis) -> None:
    part = rotation * _grooved_shaft()

    (groove,) = r.recognise_grooves(part)
    matching_steps = [
        step for step in r.recognise_turned_steps(part) if step.profile == groove.profile
    ]

    assert groove.axis == axis
    assert groove.profile is not None
    assert groove.profile.axis == axis
    assert any(step.diameter == groove.diameter for step in matching_steps)


def test_framed_groove_and_step_keep_one_profile_membership() -> None:
    part = Pos(13, -7, 5) * _grooved_shaft().rotate(Axis.X, 30)
    framed = r.build_framed_recognition_result(part, rotational=True)

    assert isinstance(framed, r.FramedRecognitionResult)
    (groove,) = framed.result.grooves
    assert groove.profile is not None
    assert any(
        step.profile == groove.profile and step.diameter == groove.diameter
        for step in framed.result.turned_steps
    )


def test_an_unclaimed_groove_cannot_suppress_a_step() -> None:
    part = _grooved_shaft()
    _, grooves, steps = _claimed(part)
    ledger = ClaimLedger(FaceGraph(part))
    ledger.propose(FamilyId.GROOVES, grooves[0])
    ledger.propose(FamilyId.TURNED_STEPS, steps[0], [ledger.graph.nodes[0]])

    assert steps_that_are_not_grooves([steps[0]], [grooves[0]], ledger.snapshot_index()) == [
        steps[0]
    ]


def test_the_ladder_keeps_the_rung_the_groove_is():
    """Both records survive in the result: a profile with a hole in it is a different shaft."""

    part = _grooved_shaft()
    product = result_module._take_inventory(part)
    result = product.result

    (groove,) = result.grooves
    rungs = [step for step in result.turned_steps if step.diameter == groove.diameter]
    assert rungs, "the ladder still has a rung at the groove's diameter"

    ladder = sorted(result.turned_steps, key=lambda step: step.lo)
    for lower, upper in zip(ladder, ladder[1:], strict=False):
        assert lower.hi == upper.lo, "and it is still contiguous"

    step_relations = [
        item
        for item in product.reconciliation.for_family(FamilyId.TURNED_STEPS)
        if item.reason is ReasonCode.TURNED_STEP_GROOVE_COMPATIBLE
    ]
    groove_relations = [
        item
        for item in product.reconciliation.for_family(FamilyId.GROOVES)
        if item.reason is ReasonCode.GROOVE_TURNED_STEP_COMPATIBLE
    ]
    assert len(step_relations) == len(groove_relations) == 1
    assert step_relations[0].outcome is groove_relations[0].outcome is Outcome.ACCEPTED
    assert step_relations[0].related == (groove_relations[0].candidate,)
    assert groove_relations[0].related == (step_relations[0].candidate,)


def test_the_rule_does_not_care_what_order_or_how_many_records_it_is_given():
    """Each step carries its own evidence, so the list is a list and not a parallel array.

    This asserted the opposite until the positional pairing went: that a short list *raises*.
    That refusal caught a count that drifted and could not see a permutation, which keeps the
    count and hands every record another record's faces.
    """

    ledger, grooves, steps = _claimed(_grooved_shaft())
    evidence = ledger.snapshot_index()
    kept = steps_that_are_not_grooves(steps, grooves, evidence)
    assert len(kept) == len(steps) - 1

    assert steps_that_are_not_grooves(list(reversed(steps)), grooves, evidence) == list(
        reversed(kept)
    )
    for one in steps:
        assert steps_that_are_not_grooves([one], grooves, evidence) == (
            [one] if one in kept else []
        )


def test_a_ledger_built_from_another_shaft_is_refused_rather_than_left_empty():
    """The provenance check, on both families, with a twin that is equal by value.

    A graph built from a different part resolves nothing, so the ledger comes back empty — and
    empty reads downstream as "these families claim nothing" rather than as "you paired the
    wrong graph", which is the reading a reconciler turns into a duplicate feature. The twin is
    the same shaft by value, so this proves provenance rather than geometry.
    """

    part = _grooved_shaft()
    twin = _grooved_shaft()
    assert r.recognise_grooves(twin) == r.recognise_grooves(part), "the twin is this shaft"

    for recognise in (r.recognise_grooves, r.recognise_turned_steps):
        foreign = ClaimLedger(FaceGraph(twin))
        try:
            recognise(part, ledger=foreign)
        except ValueError as refusal:
            assert "built from a different part" in str(refusal)
        else:
            raise AssertionError(f"{recognise.__name__} accepted another part's graph")


@pytest.mark.parametrize("factor", (1.0, 0.05, 100.0))
def test_the_census_counts_one_band_once_though_two_families_describe_it(factor):
    """The count, at three scales, with the contrast that shows the rule is not just subtracting.

    A groove adds two shoulders to the shaft and therefore two rungs to the ladder, one of which
    is the groove itself. Counting *features*, that is one more than the plain shaft has, not
    two — and it is the same answer however large the shaft is modelled, which it was not while
    the rung reported its neighbour's diameter.
    """

    grooved, plain = _grooved_shaft(), _plain_shaft()
    if factor != 1.0:
        grooved, plain = grooved.scale(factor), plain.scale(factor)

    grooved_census = r.feature_census(grooved)
    plain_census = r.feature_census(plain)

    assert grooved_census["groove"] == 1
    assert plain_census["groove"] == 0
    assert grooved_census["step"] == plain_census["step"] + 1
