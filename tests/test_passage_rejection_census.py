# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Geometry-only controls for the two-ended Passage rejection census."""

from __future__ import annotations

import math
from types import SimpleNamespace

import pytest
from build123d import (
    Box,
    BuildPart,
    BuildSketch,
    Cylinder,
    Edge,
    Plane,
    Pos,
    RegularPolygon,
    Vector,
    Wire,
    extrude,
)

from quiddity._adjacency import FaceGraph
from quiddity._section_passages import (
    _BodyAdapter,
    _enclosure_proposals,
    _line_section,
    _mouth_regions,
    section_ring_proposals,
)
from quiddity._sections import LocalFrame
from tools.audit_mfcadpp_cavity_enclosures import _two_ended_regions
from tools.audit_mfcadpp_passage_rejections import _classify_region


def _tool(sides: int):
    with BuildPart() as tool:
        with BuildSketch(Plane.XY):
            RegularPolygon(7, sides)
        extrude(amount=60, both=True)
    return tool.part


def _passage(*, interrupted: bool):
    result = Box(60, 50, 20) - _tool(6)
    return result - Pos(15, -8, 0) * Box(30, 6, 6) if interrupted else result


def _gates(part) -> list[str]:
    graph = FaceGraph(part)
    mouths = dict(_mouth_regions(graph))
    fallback = _enclosure_proposals(graph, _BodyAdapter())
    fallback_by_region = {proposal.constituent: proposal for proposal in fallback}
    final = section_ring_proposals(part, graph)
    existing_regions = frozenset(
        frozenset(proposal.nodes) for proposal in final if not proposal.constituent
    )
    final_regions = frozenset(proposal.constituent for proposal in final if proposal.constituent)
    return [
        _classify_region(
            graph,
            region,
            mouths.get(region),
            fallback_by_region,
            existing_regions,
            final_regions,
        )
        for region, _openings in _two_ended_regions(graph)
    ]


def test_census_distinguishes_new_fallback_from_existing_cycle() -> None:
    assert _gates(_passage(interrupted=True)) == ["accepted_fallback"]
    assert _gates(_passage(interrupted=False)) == ["duplicate_or_existing_cycle"]


def test_census_places_circular_bore_at_planar_seed_gate() -> None:
    assert _gates(Box(60, 50, 20) - Cylinder(7, 60)) == ["planar_mouth_seed"]


@pytest.mark.parametrize("sides", (3, 4, 6))
def test_line_section_uses_edge_incidence_not_unique_vertex_enumeration(sides: int) -> None:
    points = tuple(
        Vector(7 * math.cos(index * math.tau / sides), 7 * math.sin(index * math.tau / sides), 0)
        for index in range(sides)
    )
    edges = tuple(
        Edge.make_line(
            points[(index + 1) % sides] if index % 2 else points[index],
            points[index] if index % 2 else points[(index + 1) % sides],
        )
        for index in range(sides)
    )
    wire = Wire(edges)
    assert tuple(wire.vertices()) != tuple(edge.vertices()[0] for edge in wire.edges())

    result = _line_section(wire, LocalFrame.canonical((0.0, 0.0, 1.0), (0.0, 0.0, 0.0)))

    assert result is not None
    assert len(result[0].boundary) == sides
    assert result[0].area > 0.0


@pytest.mark.parametrize(
    "edges",
    (
        (),
        (SimpleNamespace(geom_type=SimpleNamespace(name="CIRCLE")),),
        (
            SimpleNamespace(
                geom_type=SimpleNamespace(name="LINE"),
                vertices=lambda: (object(), object()),
            ),
            SimpleNamespace(
                geom_type=SimpleNamespace(name="LINE"),
                vertices=lambda: (object(), object()),
            ),
        ),
    ),
    ids=("fewer-than-three-edges", "curved-edge", "disconnected-edges"),
)
def test_line_section_refuses_unsupported_or_malformed_boundaries(edges) -> None:
    wire = SimpleNamespace(edges=lambda: edges)

    assert (
        _line_section(  # type: ignore[arg-type]
            wire, LocalFrame.canonical((0.0, 0.0, 1.0), (0.0, 0.0, 0.0))
        )
        is None
    )
