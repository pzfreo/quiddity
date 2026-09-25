# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Identity and provenance contracts for issue #156's private candidate layer."""

from __future__ import annotations

import copy
from dataclasses import dataclass
from math import sqrt
from types import SimpleNamespace

import pytest
from build123d import Box, Cylinder, Face, Plane, Pos, Rot

from quiddity._adjacency import FaceGraph
from quiddity._candidates import (
    Candidate,
    CandidateSet,
    CompletedInputs,
    CompletedOccurrence,
    Evidence,
    FamilyId,
    Observation,
    PredicateId,
    SplitTriangularTerminalFact,
)
from quiddity._claims import ClaimLedger
from quiddity._effective_surfaces import SurfaceUse, effective_faces_for_graph
from quiddity._passage_compat import PassageCompatibilityView
from quiddity._registry import PHYSICAL_DEFINITIONS
from quiddity.angled_steps import _discover_angled_steps


@dataclass(frozen=True)
class Record:
    value: int


def _definition(family: FamilyId):
    return next(item for item in PHYSICAL_DEFINITIONS if item.family is family)


def _registry_ledger(part=None) -> ClaimLedger:
    part = Box(10, 10, 10) if part is None else part
    return ClaimLedger(FaceGraph(part), definitions=PHYSICAL_DEFINITIONS)


def test_passage_compatibility_is_required_only_for_passage_candidates() -> None:
    ledger = _registry_ledger()
    with pytest.raises(ValueError, match="require a compatibility fact"):
        ledger.sink.propose(FamilyId.PASSAGES, Record(1))

    fact = PassageCompatibilityView(None, None, None, None, None, None, False)
    with pytest.raises(ValueError, match="only passage candidates"):
        ledger.sink.propose(FamilyId.SLOTS, Record(2), compatibility=fact)

    slot = ledger.sink.propose(FamilyId.SLOTS, Record(3))
    with pytest.raises(ValueError, match="no Passage compatibility"):
        ledger.snapshot_index().passage_compatibility(slot)


def test_equal_records_remain_distinct_sink_issued_candidates() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    first_record = Record(1)
    second_record = Record(1)

    first = ledger.propose(FamilyId.LEGACY, first_record, [ledger.graph.nodes[0]])
    second = ledger.propose(FamilyId.LEGACY, second_record, [ledger.graph.nodes[1]])

    assert first_record == second_record and first_record is not second_record
    assert first is not second and first != second
    assert ledger.defining_of(first) == frozenset({ledger.graph.nodes[0]})
    assert ledger.defining_of(second) == frozenset({ledger.graph.nodes[1]})


def test_empty_evidence_is_a_candidate_but_not_a_claim() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    record = Record(1)

    candidate = ledger.propose(FamilyId.LEGACY, record)

    assert candidate.evidence.defining == frozenset()
    assert ledger.defining_of(candidate) == frozenset()
    assert ledger.defining_of(record) == frozenset()
    assert ledger.claims == ()
    assert ledger.candidate_set(FamilyId.LEGACY).candidates == (candidate,)
    assert ledger.snapshot_index().defining_of(candidate) == frozenset()


def test_constituent_evidence_defaults_to_the_exact_defining_set() -> None:
    ledger = _registry_ledger()
    candidate = ledger.sink.propose(
        FamilyId.SLOTS,
        Record(1),
        defining=[ledger.graph.nodes[0]],
    )

    assert candidate.evidence.constituent is candidate.evidence.defining
    assert ledger.snapshot_index().constituent_of(candidate) == candidate.evidence.defining


def test_circular_instance_groups_are_an_exact_same_run_partition() -> None:
    ledger = _registry_ledger()
    first, second = ledger.graph.nodes[:2]
    candidate = ledger.sink.propose(
        FamilyId.CIRCULAR_FACE_PATTERNS,
        Record(1),
        defining=[first, second],
        groups=([first], [second]),
    )
    assert ledger.snapshot_index().groups_of(candidate) == (
        frozenset({first}),
        frozenset({second}),
    )
    with pytest.raises(ValueError, match="partition"):
        ledger.sink.propose(
            FamilyId.CIRCULAR_FACE_PATTERNS,
            Record(2),
            defining=[first, second],
            groups=([first], [first]),
        )
    with pytest.raises(ValueError, match="only circular face patterns"):
        ledger.sink.propose(FamilyId.SLOTS, Record(3), defining=[first], groups=([first],))
    with pytest.raises(ValueError, match="require instance groups"):
        ledger.sink.propose(FamilyId.CIRCULAR_FACE_PATTERNS, Record(4), defining=[first])


def test_constituent_membership_is_wider_non_exclusive_and_proposal_ordered() -> None:
    ledger = _registry_ledger()
    first_node, second_node, shared = ledger.graph.nodes[:3]
    first = ledger.writer.add_defining(
        Record(1),
        [first_node],
        family=FamilyId.SLOTS,
        constituent=[first_node, shared],
    )
    second = ledger.writer.add_defining(
        Record(2),
        [second_node],
        family=FamilyId.SLOTS,
        constituent=[second_node, shared],
    )
    snapshot = ledger.snapshot_index()

    assert snapshot.constituent_of(first) == frozenset((first_node, shared))
    assert snapshot.constituent_of(second) == frozenset((second_node, shared))
    assert snapshot.constituent_of(first.record) == frozenset((first_node, shared))
    assert snapshot.constituent_of(Record(99)) == frozenset()
    assert snapshot.memberships_of(shared) == (first, second)
    assert snapshot.claims_of(shared) == ()

    foreign = FaceGraph(Box(4, 4, 4)).nodes[0]
    with pytest.raises(ValueError, match="not this graph's node"):
        snapshot.memberships_of(foreign)


def test_constituent_evidence_requires_defining_subset_and_local_nodes() -> None:
    ledger = _registry_ledger()
    other = FaceGraph(Box(4, 4, 4))
    defining, member = ledger.graph.nodes[:2]

    with pytest.raises(ValueError, match="subset"):
        ledger.sink.propose(
            FamilyId.SLOTS,
            Record(1),
            defining=[defining],
            constituent=[member],
        )
    with pytest.raises(ValueError, match="not this graph's nodes"):
        ledger.sink.propose(
            FamilyId.SLOTS,
            Record(2),
            defining=[defining],
            constituent=[defining, other.nodes[0]],
        )

    assert ledger.candidate_set(FamilyId.SLOTS).candidates == ()


def test_physical_evidence_requires_one_valid_solid_but_legacy_remains_compatible() -> None:
    assembly = Pos(-20, 0, 0) * Box(10, 10, 10) + Pos(20, 0, 0) * Box(10, 10, 10)
    ledger = ClaimLedger(FaceGraph(assembly))
    left = min(ledger.graph.nodes, key=lambda node: ledger.graph.face(node).center().X)
    right = max(ledger.graph.nodes, key=lambda node: ledger.graph.face(node).center().X)

    same_solid = ledger.propose(FamilyId.SLOTS, Record(1), [left])
    assert ledger.graph.common_valid_solid(ledger.defining_of(same_solid)) is not None
    before = ledger.candidate_set(FamilyId.SLOTS).candidates
    with pytest.raises(ValueError, match="not this graph's nodes"):
        ledger.propose(FamilyId.SLOTS, Record(2), [copy.copy(left)])
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == before
    with pytest.raises(ValueError, match="one valid closed solid"):
        ledger.propose(FamilyId.SLOTS, Record(2), [left, right])
    assert ledger.candidate_set(FamilyId.SLOTS).candidates == before

    legacy = ledger.propose(FamilyId.LEGACY, Record(3), [left, right])
    assert ledger.defining_of(legacy) == frozenset((left, right))

    open_ledger = ClaimLedger(FaceGraph(Face.make_rect(10, 10, Plane.XY)))
    with pytest.raises(ValueError, match="one valid closed solid"):
        open_ledger.propose(FamilyId.SLOTS, Record(4), open_ledger.graph.nodes)
    open_legacy = open_ledger.propose(FamilyId.LEGACY, Record(5), open_ledger.graph.nodes)
    assert open_ledger.defining_of(open_legacy) == frozenset(open_ledger.graph.nodes)


def test_surface_evidence_is_migrated_family_only_complete_unique_and_material_sided() -> None:
    part = Box(10, 10, 10)
    graph = FaceGraph(part)
    query = effective_faces_for_graph(graph)
    first, second = graph.nodes[:2]
    first_face = graph.face(first)
    second_face = graph.face(second)
    first_use = query.use(first_face)
    second_use = query.use(second_face)
    assert isinstance(first_use, SurfaceUse)
    assert isinstance(second_use, SurfaceUse)

    ledger = _registry_ledger(part)
    foreign_use = effective_faces_for_graph(graph).use(first_face)
    assert isinstance(foreign_use, SurfaceUse)
    with pytest.raises(ValueError, match="another graph run"):
        ledger.sink.propose(
            FamilyId.PADS,
            Record(0),
            defining=[ledger.graph.nodes[0]],
            surfaces=[foreign_use],
        )

    with pytest.raises(ValueError, match="only explicitly migrated"):
        ClaimLedger(graph).sink.propose(
            FamilyId.SLOTS, Record(1), defining=[first], surfaces=[first_use]
        )
    with pytest.raises(ValueError, match="repeats an original face"):
        ClaimLedger(graph).sink.propose(
            FamilyId.PADS, Record(2), defining=[first], surfaces=[first_use, first_use]
        )
    with pytest.raises(ValueError, match="cover every defining face"):
        ClaimLedger(graph).sink.propose(
            FamilyId.PADS,
            Record(3),
            defining=[first, second],
            surfaces=[first_use],
        )
    with pytest.raises(ValueError, match="exactly one material-side"):
        ClaimLedger(graph).sink.propose(
            FamilyId.PADS,
            Record(4),
            defining=[first, second],
            surfaces=[first_use, second_use],
        )
    with pytest.raises(ValueError, match="require effective-surface evidence"):
        ClaimLedger(graph).sink.propose(FamilyId.PADS, Record(5), defining=[first])

    plane_with_side = query.use(first_face, material_side=True)
    assert isinstance(plane_with_side, SurfaceUse)
    with pytest.raises(ValueError, match="incompatible material side"):
        ClaimLedger(graph).sink.propose(
            FamilyId.HOLES,
            Record(6),
            defining=[first],
            surfaces=[plane_with_side],
        )

    cylinder = Cylinder(4, 10)
    cylinder_graph = FaceGraph(cylinder)
    cylinder_query = effective_faces_for_graph(cylinder_graph)
    curved = max(cylinder.faces(), key=lambda face: face.area)
    external_use = cylinder_query.use(curved, material_side=True)
    assert isinstance(external_use, SurfaceUse)
    curved_node = cylinder_graph.require_node(curved)
    with pytest.raises(ValueError, match="incompatible material side"):
        ClaimLedger(cylinder_graph).sink.propose(
            FamilyId.HOLES,
            Record(7),
            defining=[curved_node],
            surfaces=[external_use],
        )
    with pytest.raises(ValueError, match="require effective-surface evidence"):
        ClaimLedger(cylinder_graph).sink.propose(
            FamilyId.BOSSES,
            Record(8),
            defining=[curved_node],
        )


def test_observations_freeze_without_becoming_candidates_or_claims() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    observation = ledger.sink.observe(
        FamilyId.ANGLED_STEPS,
        PredicateId.ANGLED_STEP_TERMINAL,
        subject=ledger.graph.nodes[0],
        consulted=[ledger.graph.nodes[1]],
        fact=SplitTriangularTerminalFact(4),
    )
    evidence = ledger.freeze_index()

    assert ledger.claims == ()
    assert ledger.candidate_set(FamilyId.ANGLED_STEPS).candidates == ()
    assert evidence.observations(FamilyId.ANGLED_STEPS, PredicateId.ANGLED_STEP_TERMINAL) == (
        observation,
    )
    with pytest.raises(RuntimeError, match="sealed"):
        ledger.sink.observe(
            FamilyId.ANGLED_STEPS,
            PredicateId.ANGLED_STEP_TERMINAL,
            subject=ledger.graph.nodes[0],
            consulted=[ledger.graph.nodes[1]],
            fact=SplitTriangularTerminalFact(5),
        )


def test_observation_forgery_and_post_issuance_mutation_fail_closed() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    observation = ledger.sink.observe(
        FamilyId.ANGLED_STEPS,
        PredicateId.ANGLED_STEP_TERMINAL,
        subject=ledger.graph.nodes[0],
        consulted=[ledger.graph.nodes[1]],
        fact=SplitTriangularTerminalFact(4),
    )
    evidence = ledger.snapshot_index()
    forged = object.__new__(Observation)
    for field in ("family", "predicate", "subject", "consulted", "fact", "_issuer"):
        object.__setattr__(forged, field, getattr(observation, field))
    with pytest.raises(ValueError, match="not present"):
        evidence._validate_observation(forged)

    object.__setattr__(observation, "subject", ledger.graph.nodes[2])
    with pytest.raises(ValueError, match="no longer matches"):
        ledger.snapshot_index()
    with pytest.raises(ValueError, match="no longer matches"):
        evidence.observations(FamilyId.ANGLED_STEPS, PredicateId.ANGLED_STEP_TERMINAL)


@pytest.mark.parametrize("failure", ["empty", "overlap", "family", "fact"])
def test_invalid_observations_are_rejected_atomically(failure: str) -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    subject, terminal = ledger.graph.nodes[:2]
    family = FamilyId.HOLES if failure == "family" else FamilyId.ANGLED_STEPS
    consulted = [] if failure == "empty" else [subject if failure == "overlap" else terminal]
    fact = object() if failure == "fact" else SplitTriangularTerminalFact(4)

    with pytest.raises(ValueError):
        ledger.sink.observe(
            family,
            PredicateId.ANGLED_STEP_TERMINAL,
            subject=subject,
            consulted=consulted,
            fact=fact,  # type: ignore[arg-type]
        )

    assert (
        ledger.snapshot_index().observations(
            FamilyId.ANGLED_STEPS, PredicateId.ANGLED_STEP_TERMINAL
        )
        == ()
    )


@pytest.mark.parametrize("raw_edges,effective_sides", [(3, 3), (4, 4)])
def test_split_terminal_fact_rejects_non_split_or_non_triangular_values(
    raw_edges: int, effective_sides: int
) -> None:
    with pytest.raises(ValueError, match="raw > 3 and effective == 3"):
        SplitTriangularTerminalFact(raw_edges, effective_sides)


@pytest.mark.parametrize("role", ["family", "predicate"])
def test_observation_rejects_open_enums_atomically(role: str) -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    subject = ledger.graph.nodes[0]
    family = "angled_steps" if role == "family" else FamilyId.ANGLED_STEPS
    predicate = "angled_step_terminal" if role == "predicate" else PredicateId.ANGLED_STEP_TERMINAL

    with pytest.raises(ValueError, match="closed enums"):
        ledger.sink.observe(
            family,  # type: ignore[arg-type]
            predicate,  # type: ignore[arg-type]
            subject=subject,
            consulted=[ledger.graph.nodes[1]],
            fact=SplitTriangularTerminalFact(4),
        )

    assert (
        ledger.snapshot_index().observations(
            FamilyId.ANGLED_STEPS, PredicateId.ANGLED_STEP_TERMINAL
        )
        == ()
    )


@pytest.mark.parametrize("role", ["subject", "consulted"])
def test_observation_rejects_foreign_nodes_atomically(role: str) -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    other = FaceGraph(Box(4, 4, 4))
    subject = other.nodes[0] if role == "subject" else ledger.graph.nodes[0]
    terminal = other.nodes[0] if role == "consulted" else ledger.graph.nodes[1]

    with pytest.raises(ValueError, match="not this graph's nodes"):
        ledger.sink.observe(
            FamilyId.ANGLED_STEPS,
            PredicateId.ANGLED_STEP_TERMINAL,
            subject=subject,
            consulted=[terminal],
            fact=SplitTriangularTerminalFact(4),
        )

    assert (
        ledger.snapshot_index().observations(
            FamilyId.ANGLED_STEPS, PredicateId.ANGLED_STEP_TERMINAL
        )
        == ()
    )


def test_foreign_evidence_is_refused_atomically() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    other = FaceGraph(Box(4, 4, 4))

    with pytest.raises(ValueError, match="not this graph's nodes"):
        ledger.propose(FamilyId.LEGACY, Record(1), [other.nodes[0]])

    assert ledger.candidate_set(FamilyId.LEGACY).candidates == ()
    assert ledger.claims == ()


def test_candidate_and_candidate_set_cannot_be_constructed_directly() -> None:
    with pytest.raises(TypeError):
        Candidate(FamilyId.LEGACY, Record(1), None)  # type: ignore[call-arg]
    with pytest.raises(TypeError):
        CandidateSet(FamilyId.LEGACY, ())  # type: ignore[call-arg]


def test_a_candidate_from_another_run_is_refused() -> None:
    first = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    second = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    candidate = first.propose(FamilyId.LEGACY, Record(1), [first.graph.nodes[0]])

    with pytest.raises(ValueError, match="not issued by this run"):
        second.defining_of(candidate)


@pytest.mark.parametrize("foreign", [False, True])
def test_copying_an_issuer_token_does_not_forge_a_candidate(foreign: bool) -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    other = FaceGraph(Box(4, 4, 4))
    valid = ledger.propose(FamilyId.LEGACY, Record(1), [ledger.graph.nodes[0]])
    forged = object.__new__(Candidate)
    object.__setattr__(forged, "family", FamilyId.LEGACY)
    object.__setattr__(forged, "record", Record(2))
    node = other.nodes[0] if foreign else ledger.graph.nodes[1]
    object.__setattr__(forged, "evidence", Evidence(frozenset({node})))
    object.__setattr__(forged, "_issuer", valid._issuer)

    with pytest.raises(ValueError, match="not issued by this run"):
        ledger.defining_of(forged)


@pytest.mark.parametrize(
    "field",
    [
        "foreign_evidence",
        "local_evidence",
        "evidence_contents",
        "constituent_contents",
        "family",
        "record",
    ],
)
def test_an_issued_candidate_cannot_be_altered_after_issuance(field: str) -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    other = FaceGraph(Box(4, 4, 4))
    candidate = ledger.propose(FamilyId.LEGACY, Record(1), [ledger.graph.nodes[0]])
    snapshot = ledger.snapshot_index()
    if field == "foreign_evidence":
        object.__setattr__(candidate, "evidence", Evidence(frozenset({other.nodes[0]})))
    elif field == "local_evidence":
        object.__setattr__(candidate, "evidence", Evidence(frozenset({ledger.graph.nodes[1]})))
    elif field == "evidence_contents":
        object.__setattr__(candidate.evidence, "defining", frozenset({other.nodes[0]}))
    elif field == "constituent_contents":
        object.__setattr__(candidate.evidence, "constituent", frozenset({other.nodes[0]}))
    elif field == "family":
        object.__setattr__(candidate, "family", FamilyId.ANGLED_STEPS)
    else:
        object.__setattr__(candidate, "record", Record(2))

    with pytest.raises(ValueError, match="no longer matches its issued state"):
        ledger.defining_of(candidate)
    with pytest.raises(ValueError, match="no longer matches its issued state"):
        ledger.candidate_set(FamilyId.LEGACY)
    with pytest.raises(ValueError, match="no longer matches its issued state"):
        snapshot.defining_of(candidate)


def test_direct_sink_issuance_updates_candidate_and_legacy_views() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    record = Record(1)
    candidate = ledger.sink.propose(
        FamilyId.ANGLED_STEPS,
        record,
        defining=[ledger.graph.nodes[0]],
    )

    assert ledger.candidate_set(FamilyId.ANGLED_STEPS).candidates == (candidate,)
    assert ledger.defining_of(candidate) == frozenset({ledger.graph.nodes[0]})
    assert tuple(claim.claimant for claim in ledger.claims) == (record,)
    assert ledger.claims_of(ledger.graph.nodes[0]) == ledger.claims


def test_evidence_index_is_a_point_in_time_snapshot_while_legacy_writes_continue() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    first_record = Record(1)
    first = ledger.propose(FamilyId.LEGACY, first_record, [ledger.graph.nodes[0]])

    snapshot = ledger.snapshot_index()
    second_record = Record(2)
    second = ledger.propose(FamilyId.LEGACY, second_record, [ledger.graph.nodes[1]])

    assert snapshot.candidate_set(FamilyId.LEGACY).candidates == (first,)
    assert snapshot.defining_of(first_record) == frozenset({ledger.graph.nodes[0]})
    assert snapshot.claims_of(ledger.graph.nodes[0]) == (first,)
    assert snapshot.claims_of(ledger.graph.nodes[1]) == ()
    with pytest.raises(ValueError, match="not present in this evidence snapshot"):
        snapshot.defining_of(second)

    later = ledger.snapshot_index()
    assert later.candidate_set(FamilyId.LEGACY).candidates == (first, second)
    assert later.defining_of(second) == frozenset({ledger.graph.nodes[1]})
    assert snapshot.candidate_set(FamilyId.LEGACY).candidates == (first,)


def test_evidence_capabilities_have_disjoint_runtime_surfaces() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    sink = ledger.sink
    index = ledger.snapshot_index()

    for read_name in ("candidate_set", "defining_of", "claims_of", "graph"):
        assert not hasattr(sink, read_name)
    for write_name in ("propose", "add_defining", "sink"):
        assert not hasattr(index, write_name)


def test_snapshot_preserves_duplicate_record_identity_and_rejects_ambiguous_adapter() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    record = Record(1)
    first = ledger.propose(FamilyId.LEGACY, record, [ledger.graph.nodes[0]])
    snapshot = ledger.snapshot_index()
    second = ledger.propose(FamilyId.LEGACY, record, [ledger.graph.nodes[1]])

    assert snapshot.defining_of(record) == snapshot.defining_of(first)
    with pytest.raises(ValueError, match="not present in this evidence snapshot"):
        snapshot.defining_of(second)
    with pytest.raises(ValueError, match=r"Record\(value=1\) has multiple candidates"):
        ledger.snapshot_index().defining_of(record)
    assert ledger.snapshot_index().defining_of(second) == frozenset({ledger.graph.nodes[1]})


def test_snapshot_refuses_foreign_nodes_and_candidates() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    other = ClaimLedger(FaceGraph(Box(4, 4, 4)))
    foreign = other.propose(FamilyId.LEGACY, Record(1), [other.graph.nodes[0]])
    snapshot = ledger.snapshot_index()

    with pytest.raises(ValueError, match="not present in this evidence snapshot"):
        snapshot.defining_of(foreign)
    with pytest.raises(ValueError, match="not this graph's node"):
        snapshot.claims_of(other.graph.nodes[0])


def test_terminal_freeze_closes_issuance_exactly_once() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    candidate = ledger.propose(FamilyId.HOLES, Record(1))

    evidence = ledger.freeze_index()

    assert evidence.defining_of(candidate) == frozenset()
    with pytest.raises(RuntimeError, match="issuance is sealed"):
        ledger.propose(FamilyId.HOLES, Record(2))
    with pytest.raises(RuntimeError, match="already sealed"):
        ledger.freeze_index()


def test_family_binding_reuses_claimed_candidates_and_wraps_unclaimed_occurrences() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    claimed_record = Record(1)
    claimed = ledger.propose(FamilyId.SLOTS, claimed_record, [ledger.graph.nodes[0]])
    unclaimed_record = Record(2)

    slots = ledger.candidate_set_for(FamilyId.SLOTS, [claimed_record])
    holes = ledger.candidate_set_for(FamilyId.HOLES, [unclaimed_record, unclaimed_record])

    assert slots.candidates == (claimed,)
    assert slots.candidates[0].evidence.defining == frozenset({ledger.graph.nodes[0]})
    assert len(holes.candidates) == 2
    assert holes.candidates[0] is not holes.candidates[1]
    assert all(candidate.record is unclaimed_record for candidate in holes.candidates)
    assert all(not candidate.evidence.defining for candidate in holes.candidates)


def test_family_binding_rejects_wrong_family_and_omitted_proposals() -> None:
    wrong = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    wrong_record = Record(1)
    wrong.propose(FamilyId.POCKETS, wrong_record)
    with pytest.raises(ValueError, match="issued under pockets"):
        wrong.candidate_set_for(FamilyId.SLOTS, [wrong_record])

    omitted = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    first, second = Record(1), Record(2)
    omitted.propose(FamilyId.SLOTS, first)
    omitted.propose(FamilyId.SLOTS, second)
    with pytest.raises(ValueError, match="absent from its returned inventory"):
        omitted.candidate_set_for(FamilyId.SLOTS, [first])


def test_family_completion_is_atomic_and_closes_later_issuance() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    returned, omitted = Record(1), Record(2)
    ledger.propose(FamilyId.SLOTS, omitted)

    with pytest.raises(ValueError, match="absent from its returned inventory"):
        ledger.candidate_set_for(FamilyId.SLOTS, [returned])
    issued_records = tuple(
        candidate.record for candidate in ledger.candidate_set(FamilyId.SLOTS).candidates
    )
    assert issued_records == (omitted,)

    ledger.candidate_set_for(FamilyId.SLOTS, [omitted])
    with pytest.raises(RuntimeError, match="already completed"):
        ledger.candidate_set_for(FamilyId.SLOTS, [omitted])
    with pytest.raises(RuntimeError, match="already completed"):
        ledger.propose(FamilyId.SLOTS, Record(3))

    empty = ClaimLedger(FaceGraph(Box(4, 4, 4)))
    empty.candidate_set_for(FamilyId.HOLES, ())
    with pytest.raises(RuntimeError, match="already completed"):
        empty.propose(FamilyId.HOLES, Record(4))


def test_completion_body_failure_does_not_publish_planned_empty_prefix(monkeypatch) -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    explicit, unwritten = Record(1), Record(2)
    ledger.propose(FamilyId.COUNTERSINKS, explicit, [ledger.graph.nodes[0]])

    def fail(_nodes):
        raise ValueError("solid reference changed after issuance")

    monkeypatch.setattr(ledger.graph, "common_valid_solid", fail)
    with pytest.raises(ValueError, match="solid reference changed"):
        ledger.candidate_set_for(FamilyId.COUNTERSINKS, [unwritten, explicit])
    assert tuple(
        candidate.record for candidate in ledger.candidate_set(FamilyId.COUNTERSINKS).candidates
    ) == (explicit,)


def test_completed_capabilities_cannot_be_constructed_or_broadened() -> None:
    with pytest.raises(TypeError, match="issuer-created"):
        CompletedOccurrence()
    with pytest.raises(TypeError, match="issuer-created"):
        CompletedInputs()

    ledger = _registry_ledger()
    ledger.candidate_set_for(FamilyId.COUNTERSINKS, ())
    definition = _definition(FamilyId.HOLES)
    with pytest.raises(ValueError, match="exact registered definition"):
        ledger.restricted_inputs(copy.copy(definition))
    fake = SimpleNamespace(family=FamilyId.HOLES, dependencies=(FamilyId.COUNTERSINKS,))
    with pytest.raises(ValueError, match="exact registered definition"):
        ledger.restricted_inputs(fake)

    mutable = SimpleNamespace(family=FamilyId.HOLES, dependencies=(FamilyId.COUNTERSINKS,))
    mutable_ledger = ClaimLedger(FaceGraph(Box(4, 4, 4)), definitions=(mutable,))
    mutable_ledger.candidate_set_for(FamilyId.COUNTERSINKS, ())
    inputs = mutable_ledger.restricted_inputs(mutable)
    mutable.dependencies = (FamilyId.COUNTERSINKS, FamilyId.SLOTS)
    with pytest.raises(ValueError, match="definition authority was mutated"):
        inputs.records(FamilyId.COUNTERSINKS, Record)


def test_restricted_input_validation_fails_closed_on_malformed_or_stale_state(
    monkeypatch,
) -> None:
    malformed = SimpleNamespace(family="holes", dependencies=(FamilyId.COUNTERSINKS,))
    malformed_ledger = ClaimLedger(FaceGraph(Box(3, 3, 3)), definitions=(malformed,))
    with pytest.raises(TypeError, match="physical definition authority"):
        malformed_ledger.restricted_inputs(malformed)

    recursive = SimpleNamespace(family=FamilyId.HOLES, dependencies=(FamilyId.HOLES,))
    recursive_ledger = ClaimLedger(FaceGraph(Box(3, 3, 3)), definitions=(recursive,))
    with pytest.raises(ValueError, match="dependency roster is invalid"):
        recursive_ledger.restricted_inputs(recursive)

    ledger = _registry_ledger()
    record = Record(1)
    ledger.propose(FamilyId.COUNTERSINKS, record, [ledger.graph.nodes[0]])
    ledger.candidate_set_for(FamilyId.COUNTERSINKS, (record,))
    inputs = ledger.restricted_inputs(_definition(FamilyId.HOLES))
    occurrence = inputs.occurrences(FamilyId.COUNTERSINKS, Record)[0]
    issuer = ledger._issuer

    issuer._completed_occurrences[FamilyId.COUNTERSINKS] = ()
    with pytest.raises(ValueError, match="snapshot is stale"):
        inputs.occurrences(FamilyId.COUNTERSINKS, Record)
    issuer._completed_occurrences[FamilyId.COUNTERSINKS] = (occurrence,)

    monkeypatch.setattr(ledger.graph, "common_valid_solid", lambda _nodes: None)
    with pytest.raises(ValueError, match="provenance was mutated"):
        occurrence.record(Record)

    object.__setattr__(occurrence, "_CompletedOccurrence__issuer", None)
    with pytest.raises(ValueError, match="issuer was mutated"):
        issuer.completed_defining(occurrence)
    object.__setattr__(occurrence, "_CompletedOccurrence__issuer", issuer)

    object.__setattr__(inputs, "_CompletedInputs__issuer", None)
    with pytest.raises(ValueError, match="issuer was mutated"):
        issuer.restricted_occurrences(inputs, FamilyId.COUNTERSINKS)
    with pytest.raises(ValueError, match="issuer was mutated"):
        inputs.records(FamilyId.COUNTERSINKS, Record)
    object.__setattr__(inputs, "_CompletedInputs__issuer", issuer)

    del issuer._completed[FamilyId.COUNTERSINKS]
    with pytest.raises(ValueError, match="predecessor state is stale"):
        inputs.occurrences(FamilyId.COUNTERSINKS, Record)


def test_completed_predecessor_occurrences_are_opaque_identity_provenance() -> None:
    ledger = _registry_ledger()
    record = Record(1)
    node = ledger.graph.nodes[0]
    ledger.propose(FamilyId.COUNTERSINKS, record, [node])
    ledger.candidate_set_for(FamilyId.COUNTERSINKS, [record])
    inputs = ledger.restricted_inputs(_definition(FamilyId.HOLES))

    assert inputs.records(FamilyId.COUNTERSINKS, Record) == (record,)
    (occurrence,) = inputs.occurrences(FamilyId.COUNTERSINKS, Record)
    assert occurrence.record(Record) is record
    assert occurrence.defining() == (node,)
    assert occurrence.solid() is not None
    with pytest.raises(ValueError, match="not a declared"):
        inputs.records(FamilyId.HOLES, Record)

    copied_occurrence = copy.copy(occurrence)
    with pytest.raises(ValueError, match="not issued"):
        copied_occurrence.record(Record)
    copied_inputs = copy.copy(inputs)
    with pytest.raises(ValueError, match="not issued"):
        copied_inputs.records(FamilyId.COUNTERSINKS, Record)
    with pytest.raises(TypeError):
        copy.deepcopy(inputs)
    object.__setattr__(occurrence, "_CompletedOccurrence__issuer", None)
    with pytest.raises(ValueError, match="issuer was mutated"):
        occurrence.defining()


def test_completed_empty_predecessor_has_no_invented_provenance() -> None:
    ledger = _registry_ledger()
    record = Record(1)
    ledger.candidate_set_for(FamilyId.TURNED_STEPS, [record])
    inputs = ledger.restricted_inputs(_definition(FamilyId.PLATES))
    (occurrence,) = inputs.occurrences(FamilyId.TURNED_STEPS, Record)
    assert occurrence.defining() == ()
    assert occurrence.solid() is None

    empty = _registry_ledger(Box(4, 4, 4))
    empty.candidate_set_for(FamilyId.TURNED_STEPS, ())
    assert (
        empty.restricted_inputs(_definition(FamilyId.PLATES)).occurrences(
            FamilyId.TURNED_STEPS, Record
        )
        == ()
    )


def test_terminal_inventory_rejects_foreign_or_incomplete_candidate_sets() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    record = Record(1)
    ledger.propose(FamilyId.SLOTS, record)
    slots = ledger.candidate_set_for(FamilyId.SLOTS, [record])
    evidence = ledger.freeze_index()

    other = ClaimLedger(FaceGraph(Box(4, 4, 4)))
    foreign = other.candidate_set_for(FamilyId.HOLES, [Record(2)])
    with pytest.raises(ValueError, match="another evidence issuer"):
        evidence.validate_complete_inventory((foreign,))
    with pytest.raises(ValueError, match="exactly cover"):
        evidence.validate_complete_inventory(())

    evidence.validate_complete_inventory((slots,))


def test_the_same_record_object_can_back_distinct_proposals() -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    record = Record(1)
    first = ledger.propose(FamilyId.LEGACY, record, [ledger.graph.nodes[0]])
    second = ledger.propose(FamilyId.LEGACY, record, [ledger.graph.nodes[1]])

    assert first is not second
    assert ledger.candidate_set(FamilyId.LEGACY).candidates == (first, second)
    assert ledger.defining_of(first) == frozenset({ledger.graph.nodes[0]})
    assert ledger.defining_of(second) == frozenset({ledger.graph.nodes[1]})
    with pytest.raises(ValueError, match="multiple candidates"):
        ledger.defining_of(record)
    with pytest.raises(ValueError, match="multiple candidates"):
        ledger.snapshot_index().defining_of(record)


@pytest.mark.parametrize("empty_first", [False, True])
def test_empty_and_defining_proposals_require_candidate_identity(
    empty_first: bool,
) -> None:
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    record = Record(1)
    defining = [ledger.graph.nodes[0]]
    evidence = ((), defining) if empty_first else (defining, ())
    candidates = [ledger.propose(FamilyId.LEGACY, record, nodes) for nodes in evidence]

    assert {ledger.defining_of(candidate) for candidate in candidates} == {
        frozenset(),
        frozenset(defining),
    }
    with pytest.raises(ValueError, match="multiple candidates"):
        ledger.defining_of(record)


def test_angled_steps_use_the_named_candidate_family() -> None:
    angled_step_part = Box(60, 40, 12) - Pos(-20, 20, 6) * Rot(45, 0, 0) * Box(
        30, 4 * sqrt(2), 4 * sqrt(2)
    )
    ledger = ClaimLedger(FaceGraph(angled_step_part))

    records = _discover_angled_steps(
        angled_step_part, face_edges=None, graph=ledger.graph, sink=ledger.sink
    )
    candidate_set = ledger.candidate_set(FamilyId.ANGLED_STEPS)

    assert tuple(candidate.record for candidate in candidate_set.candidates) == tuple(records)
    assert all(candidate.evidence.defining for candidate in candidate_set.candidates)
    assert tuple(claim.claimant for claim in ledger.claims) == tuple(records)
