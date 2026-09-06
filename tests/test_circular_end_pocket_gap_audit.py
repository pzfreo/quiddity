# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Authored controls for the circular-end-pocket gap audit."""

from __future__ import annotations

from build123d import Box, Compound, Cylinder, Plane, Polygon, Pos, Rot, chamfer, extrude, fillet

from quiddity._adjacency import FaceGraph
from quiddity._recess_core import _pocket_proposals_one
from quiddity._recess_faces import _cylinder_faces
from tools._legacy_recognition import (
    recognise_pockets,
)
from tools.audit_mfcadpp_circular_end_pocket_gaps import (
    _cylinder_end_result,
    _probe_component,
    _selection_hash,
    _source_selection_hash,
)


def _obround(length: float = 6, width: float = 10, depth: float = 8):
    end = Cylinder(width / 2, depth)
    return Box(length, width, depth) + Pos(-length / 2, 0, 0) * end + Pos(length / 2, 0, 0) * end


def _blind_pocket(*, angle: float = 0):
    tool = Rot(0, 0, angle) * _obround()
    return Box(60, 40, 12) - Pos(0, 0, 4) * tool


def _mouth_edges(part):
    return [
        edge
        for edge in part.edges()
        if abs(edge.bounding_box().min.Z - 6) < 0.01
        and abs(edge.bounding_box().max.Z - 6) < 0.01
        and edge.length < 20
    ]


def _partial_chamfered_mouth():
    part = _blind_pocket()
    wedge = Pos(-2, 0, 0) * extrude(
        Plane.YZ * Polygon((5, 6), (6, 6), (5, 5)),
        4,
    )
    return part - wedge


def _partial_filleted_mouth():
    # A shallow local bevel breaks the otherwise tangent-connected obround mouth into distinct
    # edge occurrences, allowing one bounded straight segment to receive a rolling treatment.
    part = _blind_pocket()
    breaker = Pos(-2, 0, 0) * extrude(
        Plane.YZ * Polygon((5, 6), (5.2, 6), (5, 5.8)),
        4,
    )
    interrupted = part - breaker
    segment = next(
        edge
        for edge in _mouth_edges(interrupted)
        if edge.geom_type.name == "LINE" and 3 < edge.length < 5
    )
    return fillet([segment], 0.1)


def _component(part, graph: FaceGraph):
    nodes = {cap[5] for cap in _cylinder_faces(part, graph) if cap[4]}
    for node in tuple(nodes):
        nodes.update(
            neighbour
            for neighbour in graph.neighbours(node)
            if graph.is_planar(neighbour) and graph.face(neighbour).area < 500
        )
    return frozenset(nodes)


def _proposal_groups(part, graph: FaceGraph):
    return tuple(
        proposal.planar
        | proposal.floors
        | frozenset(node for group in proposal.caps for node in group)
        for solid in (list(part.solids()) or [part])
        for proposal in _pocket_proposals_one(solid, graph=graph)
    )


def test_semicircular_blind_pocket_reaches_current_proposal() -> None:
    part = _blind_pocket()
    graph = FaceGraph(part)

    probe = _probe_component(part, graph, _component(part, graph), _proposal_groups(part, graph))

    assert probe.first_failed_gate == "current_proposal"
    assert probe.cylinder_faces == probe.individually_supported_ends == 2
    assert probe.principal_side_walls == 2
    assert sorted(probe.floor_counts or ()) == [0, 1]
    assert probe.cylinder_end_results == ("accepted", "accepted")


def test_full_cylinder_explains_why_a_round_hole_is_not_an_obround_end() -> None:
    part = Box(30, 30, 12) - Pos(0, 0, 4) * Cylinder(5, 10)
    graph = FaceGraph(part)
    cylinders = [cap for cap in _cylinder_faces(part, graph) if cap[4]]

    assert len(cylinders) == 1
    assert _cylinder_end_result(cylinders[0]) == "not_one_diameter_extent"


def test_partial_chamfer_does_not_explain_the_detection_gap() -> None:
    part = _partial_chamfered_mouth()
    graph = FaceGraph(part)
    proposals = _pocket_proposals_one(part, graph=graph)

    assert [(record.width, record.length, record.depth) for record in recognise_pockets(part)] == [
        (10.0, 16.0, 6.0)
    ]
    assert len(proposals) == 1
    assert len(proposals[0].constituent) > 5


def test_partial_fillet_does_not_explain_the_detection_gap() -> None:
    part = _partial_filleted_mouth()
    graph = FaceGraph(part)
    proposals = _pocket_proposals_one(part, graph=graph)

    assert [(record.width, record.length, record.depth) for record in recognise_pockets(part)] == [
        (10.0, 16.0, 6.0)
    ]
    assert len(proposals) == 1
    assert len(proposals[0].constituent) > 5


def test_complete_chamfer_and_fillet_are_existing_detection_controls() -> None:
    sharp = _blind_pocket()
    for treated in (chamfer(_mouth_edges(sharp), 1.0), fillet(_mouth_edges(sharp), 1.0)):
        assert len(recognise_pockets(treated)) == 1


def test_through_obround_is_not_a_pocket() -> None:
    through = Box(60, 40, 12) - Pos(0, 0, 0) * _obround(depth=20)

    assert recognise_pockets(through) == []


def test_incompatible_rounded_ends_do_not_false_extend_a_rectangular_pocket() -> None:
    tool = Box(12, 10, 8) + Pos(-6, 0, 0) * Cylinder(5, 8) + Pos(6, 0, 0) * Cylinder(4, 8)
    part = Box(60, 40, 12) - Pos(0, 0, 4) * tool

    assert [(record.width, record.length) for record in recognise_pockets(part)] == [(10.0, 12.0)]


def test_internally_oblique_pocket_is_not_called_a_ratio_failure() -> None:
    part = _blind_pocket(angle=10)
    graph = FaceGraph(part)

    probe = _probe_component(part, graph, _component(part, graph), _proposal_groups(part, graph))

    assert probe.first_failed_gate == "non_principal_side_walls"
    assert probe.cylinder_faces == 2
    assert probe.principal_side_walls == 0


def test_fragmented_component_stops_before_pairing() -> None:
    part = _blind_pocket()
    graph = FaceGraph(part)
    component = _component(part, graph)
    fragment = frozenset(sorted(component, key=lambda node: node.index)[:2])

    probe = _probe_component(part, graph, fragment, _proposal_groups(part, graph))

    assert probe.first_failed_gate == "fragmented_anatomy"


def test_equal_pockets_on_separate_bodies_retain_owners() -> None:
    first = _blind_pocket()
    second = Pos(100, 0, 0) * first
    compound = Compound(children=[first, second])
    graph = FaceGraph(compound)
    proposals = [
        proposal
        for solid in compound.solids()
        for proposal in _pocket_proposals_one(solid, graph=graph)
    ]

    assert len(proposals) == 2
    owners = [
        graph.common_valid_solid(
            proposal.planar
            | proposal.floors
            | frozenset(node for group in proposal.caps for node in group)
        )
        for proposal in proposals
    ]
    assert None not in owners
    assert owners[0] != owners[1]


def test_selection_hashes_pin_order_and_source_content() -> None:
    assert _selection_hash(["100", "200"]) != _selection_hash(["200", "100"])
    assert _source_selection_hash([("100", "aaa")]) != _source_selection_hash([("100", "changed")])
