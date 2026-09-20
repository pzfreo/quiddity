# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""One node per face: the attributes seven modules derive privately, and who claimed what.

Two properties carry this class, and neither is a golden.

The first is that it is the *same* answer the free functions already give. ``FaceGraph`` is
only worth having if a recogniser can move onto it without its output moving, so the
attribute and adjacency tests here compare it against the derivation it replaces on a part
carrying real features, rather than against a captured expectation.

The second is that the graph is geometric fact and nothing else. It carries no ownership: who
claimed which face is an interpretation, and lives in the separate ledger tested in
:mod:`tests.test_claims`.
"""

from __future__ import annotations

import copy
import dataclasses

import pytest
from build123d import Axis, Box, Cylinder, Keep, Plane, Pos, chamfer, fillet

from quiddity._adjacency import FaceEdges, FaceGraph, edge_face_map, neighbours


def featured_part():
    """A part with a hole, a slot, a chamfer and a fillet, so no node is a bare box face."""

    part = Box(60, 40, 12) - Pos(-18, 0, 0) * Cylinder(4, 12) - Pos(15, 0, 0) * Box(24, 8, 12)
    part = chamfer(part.edges().filter_by(Axis.Z).group_by(Axis.X)[0], 1.5)
    return fillet(part.edges().filter_by(Axis.Z).group_by(Axis.X)[-1], 2.0)


def test_a_face_from_a_second_walk_resolves_to_the_same_node():
    """The property the whole graph rests on, stated directly.

    Every consumer reaches the graph holding faces it got from its own ``part.faces()`` call,
    so if a second walk produced faces the index did not recognise, every lookup would return
    None and the graph would silently do nothing. Asserting the count first stops a part that
    yielded nothing from passing this vacuously.
    """

    part = featured_part()
    graph = FaceGraph(part)
    second = part.faces()

    assert len(second) == len(graph) > 10
    assert {graph.node_of(face) for face in second} == set(graph.nodes)


def test_is_same_equal_face_occurrences_fail_closed() -> None:
    """Opposite orientations of one shared face must not silently resolve to one node."""

    lower, upper = Box(2, 2, 2).split(Pos(1, 0, 0) * Plane.YZ, keep=Keep.BOTH)
    faces = [*lower.faces(), *upper.faces()]
    duplicates = [
        (left, right) for at, left in enumerate(faces) for right in faces[at + 1 :] if left == right
    ]
    assert len(faces) == 12 and len(set(faces)) == 11
    assert len(duplicates) == 1
    left, right = duplicates[0]
    assert left.wrapped.IsSame(right.wrapped)
    assert not left.wrapped.IsEqual(right.wrapped)

    class SharedFaceTraversal:
        def faces(self):
            return faces

    with pytest.raises(ValueError, match="IsSame-equal occurrences"):
        FaceGraph(SharedFaceTraversal())


def test_a_face_of_another_part_has_no_node():
    """None rather than KeyError, so a caller reconciling across parts gets an answer."""

    assert FaceGraph(Box(10, 10, 10)).node_of(Box(4, 4, 4).faces()[0]) is None


def test_a_node_of_another_graph_is_refused_by_every_accessor():
    """Provenance, which a bare integer index could not carry.

    Node 0 of one part is a perfectly valid index into another, so a foreign node would have
    been accepted and would have silently answered about the wrong face -- the accidental
    identity this substrate exists to remove. The node is genuinely valid; it just belongs
    somewhere else, which is the case a bounds check cannot see.
    """

    mine = FaceGraph(Box(10, 10, 10))
    theirs = FaceGraph(Box(60, 40, 12) - Pos(15, 0, 0) * Box(24, 8, 12))
    foreign = theirs.nodes[0]

    assert 0 <= foreign.index < len(mine), "the foreign node must be in range to prove anything"
    assert not mine.owns(foreign)
    for ask in (mine.face, mine.edges, mine.surface, mine.normal, mine.bounds, mine.neighbours):
        with pytest.raises(ValueError, match="not issued by this graph"):
            ask(foreign)


def test_a_node_handle_cannot_be_invalidated():
    """A writable index would let a caller break a handle the graph itself issued.

    ``owns`` would then refuse a node that genuinely came from this graph, which is the same
    class of silent wrongness as accepting one that did not.
    """

    graph = FaceGraph(Box(10, 10, 10))
    node = graph.nodes[0]
    with pytest.raises(dataclasses.FrozenInstanceError):
        node.index = 1

    assert graph.owns(node)
    assert graph.face(node) is graph.face(graph.nodes[0])


def test_the_edges_handed_out_cannot_be_mutated():
    """The graph is immutable through its own API, not by request in a docstring.

    A returned list was the memo's own, so clearing it would have changed adjacency,
    ``shared_edges`` and every later answer about that face.
    """

    graph = FaceGraph(featured_part())
    node = graph.nodes[0]
    assert isinstance(graph.edges(node), tuple)
    assert isinstance(graph.neighbours(node), tuple)


def test_attributes_agree_with_the_derivations_they_replace():
    """Every node's attributes match the direct kernel call the seven modules make today.

    This is the migration safety property: a recogniser that swaps ``face.bounding_box()`` for
    ``graph.bounds(node)`` must not change its own answer.
    """

    part = featured_part()
    graph = FaceGraph(part)

    for node in graph.nodes:
        face = graph.face(node)
        box = face.bounding_box()
        assert graph.bounds(node) == (
            (box.min.X, box.max.X),
            (box.min.Y, box.max.Y),
            (box.min.Z, box.max.Z),
        )
        assert graph.is_planar(node) == (face.geom_type.name == "PLANE")

        unit = face.normal_at()
        assert graph.normal(node) == (unit.X, unit.Y, unit.Z)
        assert set(graph.edges(node)) == set(face.edges())
        assert len(graph.edges(node)) == len(face.edges())

    assert any(not graph.is_planar(node) for node in graph.nodes), "no curved face to check"


def test_adjacency_agrees_with_the_free_function():
    """The graph's neighbours are the existing helper's neighbours, node for node."""

    part = featured_part()
    graph = FaceGraph(part)
    faces = part.faces()
    by_edge = edge_face_map(faces)

    for node in graph.nodes:
        expected = {graph.node_of(face) for face in neighbours(graph.face(node), by_edge)}
        assert set(graph.neighbours(node)) == expected
        assert len(graph.neighbours(node)) == len(expected), "a neighbour was reported twice"

    assert any(graph.neighbours(node) for node in graph.nodes)


def test_shared_edges_are_non_empty_exactly_for_neighbours():
    """Adjacency and the edges that cause it are one answer, not two that could disagree."""

    graph = FaceGraph(featured_part())
    checked = 0
    for node in graph.nodes:
        for other in graph.nodes:
            if other == node:
                continue
            shared = graph.shared_edges(node, other)
            assert bool(shared) == (other in graph.neighbours(node))
            checked += bool(shared)
    assert checked


def test_attributes_are_not_derived_until_asked():
    """Laziness is the documented reason this costs nothing to construct.

    Measured, sharing the walk over ``part.faces()`` is worth 1.9% of a census while the
    derivations hanging off it are worth far more — so a graph built by a recogniser that only
    wants adjacency must not pay for normals, surfaces or bounds.
    """

    graph = FaceGraph(featured_part())
    assert (graph._normal, graph._surface, graph._bounds, graph._edges) == ({}, {}, {}, {})

    graph.normal(graph.nodes[0])
    assert graph._normal and not graph._surface and not graph._bounds


def test_an_injected_face_edges_memo_is_the_one_actually_used():
    """ADR 0002's rule: prove the injected dependency is used by making it answer differently.

    A graph that quietly built its own memo would still pass every other test here while
    paying the ``Face.edges()`` cost a second time across a census, which is the whole reason
    the memo is threaded.
    """

    class Truncating(FaceEdges):
        def of(self, face):
            return super().of(face)[:1]

    part = featured_part()
    plain = FaceGraph(part)
    assert all(len(plain.edges(node)) > 1 for node in plain.nodes)

    injected = FaceGraph(part, face_edges=Truncating())
    assert all(len(injected.edges(node)) == 1 for node in injected.nodes)


def test_common_valid_solid_is_run_owned_and_fails_closed() -> None:
    assembly = Pos(-20, 0, 0) * Box(10, 10, 10) + Pos(20, 0, 0) * Box(10, 10, 10)
    graph = FaceGraph(assembly)
    left = tuple(node for node in graph.nodes if graph.face(node).center().X < 0)
    right = tuple(node for node in graph.nodes if graph.face(node).center().X > 0)

    solid = graph.common_valid_solid(left)
    assert solid is not None
    assert graph.common_valid_solid(left[:2]) is solid
    assert graph.common_valid_solid(()) is None
    assert graph.common_valid_solid((left[0], right[0])) is None

    foreign = FaceGraph(Box(10, 10, 10))
    with pytest.raises(ValueError, match="not issued by this graph"):
        graph.common_valid_solid((foreign.nodes[0],))
    with pytest.raises(ValueError, match="not issued by this graph"):
        graph.common_valid_solid((copy.copy(left[0]),))

    object.__setattr__(solid, "ordinal", 99)
    with pytest.raises(ValueError, match="solid reference changed"):
        graph.common_valid_solid(left)


def test_common_valid_solid_refuses_ambiguous_membership() -> None:
    graph = FaceGraph(Box(10, 10, 10))
    graph._build_solid_ownership()
    memberships = list(graph._face_solids)
    memberships[0] = (0, 1)
    graph._face_solids = tuple(memberships)
    graph._closed_solids = frozenset({0, 1})

    assert graph.common_valid_solid((graph.nodes[0],)) is None
