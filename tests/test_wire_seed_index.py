# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Opening-wire incidence read from the graph's edge index: same seeds, asked once.

Three families walk every node's inner wires and ask which neighbours carry each wire. That
question used to be answered by a scan -- every neighbour, every shared occurrence, every edge
of the wire, compared with build123d's ``==``. It is now one lookup in an index the graph builds
once per face.

Two properties carry the change, and neither is a golden. The first is that the seed is the
*same* frozenset the scan returned, which the equivalence tests below check by running the
original scan beside the index over every wire of every face of a real part. The second is that
the index is actually built once, which the operation-count sentinel at the bottom pins.
"""

from __future__ import annotations

from pathlib import Path

import pytest
from build123d import Box, Location, Pos

from quiddity import _recess_core, _section_passages, prismatic_pockets
from quiddity._adjacency import FaceGraph, FaceNode
from quiddity._wire_seed import wire_seed
from quiddity.census import feature_census
from quiddity.step_io import import_step_geometry

CORPUS = Path(__file__).parent / "corpus"

#: The part the performance analysis measured this scan on: 158 faces, and the openings whose
#: inner wires the three families ask about have most of the part for neighbours.
_SENTINEL_PART = CORPUS / "nist" / "nist_ftc_09_asme1_rd.stp"


def _scanned_seed(graph: FaceGraph, opening: FaceNode, wire) -> frozenset[FaceNode]:
    """The nested scan this replaced, kept verbatim as the thing the index must agree with."""

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


def _pocketed_part():
    """A box with two blind pockets, so its floors and its top have real inner wires."""

    return (
        Box(60, 40, 20) - Pos(-15, 0, 5) * Box(16, 12, 12) - Location((18, 4, 5)) * Box(10, 10, 12)
    )


@pytest.fixture(scope="module")
def sentinel_graph() -> FaceGraph:
    return FaceGraph(import_step_geometry(_SENTINEL_PART))


@pytest.mark.parametrize("build", [_pocketed_part, lambda: Box(10, 10, 10)])
def test_every_seed_is_the_seed_the_neighbour_scan_returned(build) -> None:
    graph = FaceGraph(build())
    seen = 0
    for opening in graph.nodes:
        for wire in graph.face(opening).wires():
            assert wire_seed(graph, opening, wire) == _scanned_seed(graph, opening, wire)
            seen += 1
    assert seen >= len(graph)  # every face has at least its outer wire


def test_a_real_part_seeds_identically_on_every_wire_of_every_face(sentinel_graph) -> None:
    """The equivalence that matters, on the part the analysis named.

    Imported geometry is where the two identities could have come apart: a wire's edge and the
    occurrence's edge are wrappers read from two different faces, with opposite orientations.
    """

    non_empty = 0
    for opening in sentinel_graph.nodes:
        for wire in sentinel_graph.face(opening).wires():
            expected = _scanned_seed(sentinel_graph, opening, wire)
            assert wire_seed(sentinel_graph, opening, wire) == expected
            non_empty += bool(expected)
    assert non_empty > 100  # the fixture would be vacuous if the scan found nothing


def test_an_edge_read_from_the_neighbour_finds_the_entry_the_face_created() -> None:
    """Orientation is not part of the identity, here as in ``shared_edges``.

    The index is keyed on the ``Edge`` an occurrence carries, which is read from whichever of
    the two faces has the lower node index. A wire of the *other* face hands back the same edge
    reversed, and it must still hit the entry -- that is exactly what ``IsSame`` means, and it
    is the whole reason a dict lookup can stand in for the ``==`` scan.
    """

    graph = FaceGraph(Box(10, 10, 10))
    node = graph.nodes[0]
    index = graph.neighbours_by_occurrence_edge(node)
    reversed_here = 0
    for neighbour in graph.neighbours(node):
        for edge in graph.face(neighbour).edges():
            if edge not in index:
                continue
            assert neighbour in index[edge]
            key = next(k for k in index if k == edge)
            assert hash(key) == hash(edge)
            reversed_here += key.wrapped.Orientation() != edge.wrapped.Orientation()
    assert reversed_here  # the fixture must actually exercise the reversed reading


def test_the_index_reports_exactly_the_paired_shared_occurrences(sentinel_graph) -> None:
    """It says no more than ``shared_occurrences`` does, and no less.

    A pair that meets along an edge with no traversal-independent pairing has no occurrence,
    so it contributes no entry -- which is why the index cannot be built from ``shared_edges``.
    """

    for node in sentinel_graph.nodes:
        expected: dict = {}
        for neighbour in sentinel_graph.neighbours(node):
            for occurrence in sentinel_graph.shared_occurrences(node, neighbour):
                expected.setdefault(occurrence.edge, set()).add(neighbour)
        index = sentinel_graph.neighbours_by_occurrence_edge(node)
        assert {edge: set(carriers) for edge, carriers in index.items()} == expected


def test_the_index_is_this_graph_s_and_cannot_be_edited(sentinel_graph) -> None:
    node = sentinel_graph.nodes[0]
    index = sentinel_graph.neighbours_by_occurrence_edge(node)
    assert index is sentinel_graph.neighbours_by_occurrence_edge(node)
    with pytest.raises(TypeError):
        index[next(iter(index))] = ()  # type: ignore[index]
    with pytest.raises(ValueError, match="not issued by this graph"):
        sentinel_graph.neighbours_by_occurrence_edge(FaceGraph(Box(1, 1, 1)).nodes[0])


#: One census of the sentinel part, before and after the index.
#:
#: 280 wire seeds are answered, over two opening faces. The scan read each of those faces'
#: complete neighbour-by-neighbour occurrence lists once per seed; the index reads them once per
#: face. The seeds themselves are unchanged -- that is the equivalence above -- and the census
#: went from 3.0 s to 2.8 s.
_WIRE_SEEDS = 280
_OCCURRENCE_READS_BEFORE = 36528
_OCCURRENCE_READS = 260

#: The three aliases the families bind at import; ``tests/test_shared_geometry_primitives``
#: pins that they are one function, and counting means wrapping each binding.
_SEED_BINDINGS = (
    (prismatic_pockets, "_wire_seed"),
    (_section_passages, "_wire_seed"),
    (_recess_core, "_inner_wire_seed"),
)


def test_one_census_reads_each_opening_s_occurrences_once(monkeypatch) -> None:
    """An operation-count sentinel, not a wall-clock one.

    A family that goes back to scanning neighbours, or an index that stops being cached, leaves
    every answer correct and only makes the run slower -- counting the graph reads is the only
    way that shows up in CI.
    """

    seeds: list[int] = []
    reads: list[int] = []
    building = [0]
    original_index = FaceGraph.neighbours_by_occurrence_edge
    original_occurrences = FaceGraph.shared_occurrences

    def counted_index(self, node):
        building[0] += 1
        try:
            return original_index(self, node)
        finally:
            building[0] -= 1

    def counted_occurrences(self, a, b):
        if building[0]:
            reads.append(1)
        return original_occurrences(self, a, b)

    def counted_seed(graph, opening, wire):
        seeds.append(1)
        return wire_seed(graph, opening, wire)

    monkeypatch.setattr(FaceGraph, "neighbours_by_occurrence_edge", counted_index)
    monkeypatch.setattr(FaceGraph, "shared_occurrences", counted_occurrences)
    for module, attribute in _SEED_BINDINGS:
        monkeypatch.setattr(module, attribute, counted_seed)

    feature_census(import_step_geometry(_SENTINEL_PART))

    assert len(seeds) == _WIRE_SEEDS
    assert len(reads) == _OCCURRENCE_READS, (
        f"a census of {_SENTINEL_PART.name} answered {len(seeds)} wire seeds with "
        f"{len(reads)} occurrence-list reads; {_OCCURRENCE_READS} is the count once each "
        f"opening's index is built once, and {_OCCURRENCE_READS_BEFORE} was the count when "
        "every seed rescanned every neighbour. A rise means a seed stopped going through "
        "FaceGraph.neighbours_by_occurrence_edge; a fall means this sentinel needs updating."
    )
