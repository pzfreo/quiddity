# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Shared physical wire incidence; traversal and feature acceptance stay with callers."""

from build123d import Wire

from quiddity._adjacency import FaceGraph, FaceNode


def wire_seed(graph: FaceGraph, opening: FaceNode, wire: Wire) -> frozenset[FaceNode]:
    """Return neighbouring source faces sharing an edge of the supplied opening wire."""
    edges = tuple(wire.edges())
    return frozenset(
        neighbour
        for neighbour in graph.neighbours(opening)
        if any(
            occurrence.edge == edge
            for occurrence in graph.shared_occurrences(opening, neighbour)
            for edge in edges
        )
    )
