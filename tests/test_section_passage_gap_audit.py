# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Authored controls for the section-passage gap audits."""

from __future__ import annotations

import pytest
from build123d import (
    Box,
    BuildPart,
    BuildSketch,
    Compound,
    Cylinder,
    Plane,
    Pos,
    RegularPolygon,
    Rot,
    extrude,
)

from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._section_passages import section_ring_proposals
from quiddity.passages import _discover_section_passages
from tools.audit_mfcadpp_section_passage_gaps import (
    _KNOWN_INVALID_REASON,
    _invalid_row,
    _probe_component,
    _relation,
    _selection_hash,
    _source_selection_hash,
)


def _polygonal_tool(sides: int, *, depth: float = 60.0):
    with BuildPart() as tool:
        with BuildSketch(Plane.XY):
            RegularPolygon(7, sides)
        extrude(amount=depth, both=depth == 60.0)
    return tool.part


def _polygonal_passage(sides: int = 6):
    return Box(60, 40, 20) - _polygonal_tool(sides)


def _interrupted_polygonal_passage(sides: int):
    passage = Box(60, 50, 20) - _polygonal_tool(sides)
    if sides == 3:
        return passage - Pos(15, 3, 5) * Box(30, 8, 6)
    return passage - Pos(15, -8, 0) * Box(30, 6, 6)


def _vertical_inner_walls(graph: FaceGraph):
    return frozenset(
        node
        for node in graph.nodes
        if graph.is_planar(node)
        and (normal := graph.normal(node)) is not None
        and abs(normal[2]) < 1e-8
        and graph.face(node).area < 200.0
    )


@pytest.mark.parametrize("sides", (3, 4, 6))
def test_intact_polygonal_passage_reaches_exact_production_proposal(sides: int) -> None:
    part = _polygonal_passage(sides)
    graph = FaceGraph(part)
    (proposal,) = section_ring_proposals(part, graph)
    component = frozenset(proposal.nodes)

    probe = _probe_component(graph, component)

    assert probe.first_failed_gate == "recognisable"
    assert probe.planar_walls == probe.collinear_pairs == probe.interval_pairs == sides
    assert probe.cycle_faces == sides
    assert component == frozenset(proposal.nodes)


@pytest.mark.parametrize("sides", (3, 4, 6))
def test_two_mouth_enclosure_recovers_interrupted_polygonal_passage(
    sides: int, monkeypatch
) -> None:
    import quiddity._section_passages as module

    part = _interrupted_polygonal_passage(sides)
    graph = FaceGraph(part)
    (proposal,) = section_ring_proposals(part, graph)

    assert len(proposal.section.boundary) == sides
    assert proposal.constituent == frozenset(proposal.nodes)
    assert graph.common_valid_solid(proposal.constituent) is proposal.solid

    monkeypatch.setattr(module, "_enclosure_proposals", lambda *_args: ())
    assert module.section_ring_proposals(part, FaceGraph(part)) == ()


@pytest.mark.parametrize("sides", (3, 4, 6))
def test_two_mouth_enclosure_is_rigid_transform_covariant(sides: int) -> None:
    part = _interrupted_polygonal_passage(sides)
    moved = Pos(17, -11, 9) * Rot(31, 17, 23) * part

    base = section_ring_proposals(part, FaceGraph(part))
    transformed = section_ring_proposals(moved, FaceGraph(moved))

    assert len(base) == len(transformed) == 1
    assert len(base[0].section.boundary) == len(transformed[0].section.boundary) == sides
    assert base[0].run_interval[1] - base[0].run_interval[0] == pytest.approx(
        transformed[0].run_interval[1] - transformed[0].run_interval[0]
    )


def test_two_mouth_fallback_preserves_equal_compound_occurrences() -> None:
    first = _interrupted_polygonal_passage(6)
    compound = Compound([first, Pos(100, 0, 0) * first])
    graph = FaceGraph(compound)

    proposals = section_ring_proposals(compound, graph)

    assert len(proposals) == 2
    assert all(proposal.constituent for proposal in proposals)
    assert proposals[0].solid is not proposals[1].solid


def test_two_mouth_fallback_publishes_exact_constituent_membership() -> None:
    part = _interrupted_polygonal_passage(6)
    graph = FaceGraph(part)
    ledger = ClaimLedger(graph)
    (proposal,) = section_ring_proposals(part, graph)

    (record,) = _discover_section_passages(part, ledger.graph, ledger.sink)
    (candidate,) = ledger.candidate_set(FamilyId.PASSAGES).candidates
    evidence = ledger.snapshot_index()

    assert candidate.record is record
    assert evidence.defining_of(candidate) == frozenset(proposal.nodes)
    assert evidence.constituent_of(candidate) == proposal.constituent
    assert evidence.defining_of(candidate) <= evidence.constituent_of(candidate)


def test_two_mouth_fallback_refuses_blind_and_circular_voids() -> None:
    import quiddity._section_passages as module

    blind = Box(60, 50, 20) - _polygonal_tool(6, depth=10)
    circular = Box(60, 50, 20) - Cylinder(7, 60)

    for part in (blind, circular):
        graph = FaceGraph(part)
        assert module._enclosure_proposals(graph, module._BodyAdapter()) == ()


def test_two_mouth_fallback_adds_no_occurrence_to_existing_crossed_cycles() -> None:
    import quiddity._section_passages as module

    branched = Box(60, 50, 20) - Box(16, 12, 60) - Box(60, 8, 6)
    graph = FaceGraph(branched)

    fallback = module._enclosure_proposals(graph, module._BodyAdapter())
    final = section_ring_proposals(branched, graph)

    assert {frozenset(proposal.nodes) for proposal in fallback} == {
        frozenset(proposal.nodes) for proposal in final
    }
    assert all(not proposal.constituent for proposal in final)


@pytest.mark.parametrize("sides", (3, 4, 6))
def test_capped_polygonal_void_reaches_only_material_or_capped_gate(sides: int) -> None:
    part = Box(60, 40, 20) - _polygonal_tool(sides, depth=10.0)
    graph = FaceGraph(part)
    component = _vertical_inner_walls(graph)

    probe = _probe_component(graph, component)

    assert len(component) == sides
    assert probe.first_failed_gate == "material_or_capped"
    assert section_ring_proposals(part, graph) == ()


@pytest.mark.parametrize("sides", (4, 6))
def test_probe_is_axis_covariant(sides: int) -> None:
    first_part = _polygonal_passage(sides)
    second_part = Rot(90, 0, 0) * first_part
    first_graph = FaceGraph(first_part)
    second_graph = FaceGraph(second_part)
    (first,) = section_ring_proposals(first_part, first_graph)
    (second,) = section_ring_proposals(second_part, second_graph)

    first_probe = _probe_component(first_graph, frozenset(first.nodes))
    second_probe = _probe_component(second_graph, frozenset(second.nodes))

    assert first_probe.first_failed_gate == second_probe.first_failed_gate == "recognisable"
    assert first_probe.run != second_probe.run
    assert first_probe.planar_walls == second_probe.planar_walls == sides


def test_equal_rectangular_passages_on_separate_bodies_remain_distinct() -> None:
    first = _polygonal_passage(4)
    second = Pos(100, 0, 0) * first
    compound = Compound(children=[first, second])
    graph = FaceGraph(compound)
    proposals = section_ring_proposals(compound, graph)

    assert len(proposals) == 2
    components = [frozenset(proposal.nodes) for proposal in proposals]
    assert all(
        _probe_component(graph, component).first_failed_gate == "recognisable"
        for component in components
    )
    owners = [graph.common_valid_solid(component) for component in components]
    assert None not in owners
    assert owners[0] != owners[1]


def test_audit_keeps_defining_and_constituent_coverage_separate() -> None:
    part = _polygonal_passage(4)
    graph = FaceGraph(part)
    (proposal,) = section_ring_proposals(part, graph)
    component = frozenset(proposal.nodes)
    defining = frozenset((min(component, key=lambda node: node.index),))
    claims = (("passages", defining, component),)

    defining_relation = _relation(component, claims, 1)
    constituent_relation = _relation(component, claims, 2)

    assert defining_relation["covered_faces"] == 1
    assert defining_relation["full"] is False
    assert constituent_relation["covered_faces"] == 4
    assert constituent_relation["full"] is True


def test_selection_hashes_pin_order_and_source_content() -> None:
    assert _selection_hash(["100", "200"]) == _selection_hash(["100", "200"])
    assert _selection_hash(["100", "200"]) != _selection_hash(["200", "100"])
    assert _source_selection_hash([("100", "aaa")]) != _source_selection_hash([("100", "changed")])


def test_audit_records_only_the_documented_invalid_model_and_reason() -> None:
    error = ValueError(_KNOWN_INVALID_REASON)

    assert _invalid_row("14052", "source", error, allow_invalid=True) == {
        "model_id": "14052",
        "source_sha256": "source",
        "reason": _KNOWN_INVALID_REASON,
    }
    with pytest.raises(ValueError, match=_KNOWN_INVALID_REASON):
        _invalid_row("14052", "source", error, allow_invalid=False)
    with pytest.raises(ValueError, match="different"):
        _invalid_row("14052", "source", ValueError("different"), allow_invalid=True)
    with pytest.raises(ValueError, match=_KNOWN_INVALID_REASON):
        _invalid_row("not-documented", "source", error, allow_invalid=True)
