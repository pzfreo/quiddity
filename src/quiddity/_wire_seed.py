# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Shared physical wire incidence; traversal and feature acceptance stay with callers."""

from build123d import Wire

from quiddity._adjacency import FaceGraph, FaceNode


def wire_seed(graph: FaceGraph, opening: FaceNode, wire: Wire) -> frozenset[FaceNode]:
    """Return neighbouring source faces sharing an edge of the supplied opening wire.

    The question is asked of one face many times -- three families walk every node's inner
    wires, and a face with several wires is asked once per wire -- so it is asked of the
    graph's own edge index rather than by scanning. The scan this replaced compared every
    neighbour's every shared occurrence against every edge of the wire: on a 158-face NIST
    part, 36,528 occurrence-list reads for 280 answers, where the index needs 260.

    :meth:`FaceGraph.neighbours_by_occurrence_edge` holds exactly the occurrences that scan
    read, keyed by the live ``Edge`` the occurrence carries, and a ``dict`` lookup on a
    build123d shape *is* the ``==`` the scan performed -- ``hash`` of the ``TopoDS_Shape``
    narrowed by ``IsSame``. So the same neighbours come back, for the same reason.
    """

    sharing = graph.neighbours_by_occurrence_edge(opening)
    if not sharing:
        return frozenset()
    return frozenset(neighbour for edge in wire.edges() for neighbour in sharing.get(edge, ()))
