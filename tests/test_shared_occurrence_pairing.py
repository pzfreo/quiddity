# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Pairing two faces' edge occurrences by grouping them, not by scanning them.

``FaceGraph.shared_occurrences`` has to split each face's oriented half-edges into groups that
run along the same edge, then pair the two faces' groups. It used to do that with nested
``IsSame`` scans: pop a seed, walk every other pending left half and ``list.remove`` the matches,
then walk every right half again for the same seed. That is ``O(L² + L×R)`` ``IsSame`` calls per
adjacent pair, and on a 664-face part with 120 half-edges on a single face it dominated: a census
of ``nist_ctc_02`` spent 1,147,399 of its 1,211,185 ``TopoDS_Shape.IsSame`` calls inside this one
method.

It is now one ordered pass per face into a dict keyed on the live ``Edge`` wrapper -- the same
``hash``-is-the-shape's, ``__eq__``-is-``IsSame`` identity ``neighbours_by_occurrence_edge`` and
``shared_edges`` already index on.

Two properties carry the change. The first is that the pairing is *the same pairing*, which the
equivalence tests below check by running the original scan, copied verbatim, beside the method
over every adjacent pair of every vendored corpus part -- the one test here marked ``slow``, for
the reason recorded on it. The second is that the scan is really gone, which the operation-count
sentinel at the bottom pins, and which stays in the fast tier because it is the thing that fails
when this work is undone.
"""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import pytest
from build123d import Box, Cylinder, Part, Sphere, Torus
from OCP.TopoDS import TopoDS_Shape

from quiddity._adjacency import EdgeOccurrenceRef, FaceGraph, FaceNode
from quiddity._typing import EdgeLike
from quiddity.census import feature_census
from quiddity.step_io import import_step_geometry

CORPUS = Path(__file__).parent / "corpus"

CORPUS_PARTS = sorted(
    path for path in CORPUS.rglob("*") if path.suffix in {".stp", ".step"} and path.is_file()
)

#: The part the round-2 analysis measured this on: 664 faces, 1,302 adjacent pairs, and the
#: single face whose 120 half-edges made the left-hand scan quadratic.
_SENTINEL_PART = CORPUS / "nist" / "nist_ctc_02_asme1_rc.stp"


def _scanned_groups(
    halves: tuple[EdgeOccurrenceRef, ...],
) -> list[list[EdgeOccurrenceRef]]:
    """The pop/scan/``remove`` grouping loop this replaced, kept verbatim to agree with."""

    groups: list[list[EdgeOccurrenceRef]] = []
    pending_left = list(halves)
    while pending_left:
        seed = pending_left.pop(0)
        left_group = [seed]
        for half in tuple(pending_left):
            if half.edge.wrapped.IsSame(seed.edge.wrapped):
                pending_left.remove(half)
                left_group.append(half)
        groups.append(left_group)
    return groups


#: A parametrised fixture's geometry, built inside the test so each case gets its own solid.
_Build = Callable[[], Part]

#: What ``_scanned_pairing`` reports in place of a ``SharedEdgeOccurrenceRef``: the endpoints,
#: the two halves and the edge the method would have constructed one from.
_ScannedPair = tuple[
    tuple[FaceNode, FaceNode], tuple[EdgeOccurrenceRef, EdgeOccurrenceRef], EdgeLike
]


def _scanned_pairing(graph: FaceGraph, a: FaceNode, b: FaceNode) -> list[_ScannedPair]:
    """The whole original algorithm, verbatim, reporting what it would have issued.

    Verbatim except for the issuing itself: it yields the three fields the method puts in each
    ``SharedEdgeOccurrenceRef`` rather than constructing one, so that running it beside the
    method does not register occurrences in the graph's ledger and change what the method
    under test is asked about.
    """

    at_a, at_b = graph._at(a), graph._at(b)
    if at_a == at_b:
        return []
    left, right = (a, b) if at_a < at_b else (b, a)
    left_halves = graph._face_edge_occurrences(left)
    right_halves = graph._face_edge_occurrences(right)
    pairs: list[_ScannedPair] = []
    pending_left = list(left_halves)
    while pending_left:
        seed = pending_left.pop(0)
        left_group = [seed]
        for half in tuple(pending_left):
            if half.edge.wrapped.IsSame(seed.edge.wrapped):
                pending_left.remove(half)
                left_group.append(half)
        right_group = [half for half in right_halves if half.edge.wrapped.IsSame(seed.edge.wrapped)]
        candidate_pairs = {
            left_half: tuple(
                right_half
                for right_half in right_group
                if right_half.reversed is not left_half.reversed
            )
            for left_half in left_group
        }
        reverse_counts = {
            right_half: sum(right_half in matches for matches in candidate_pairs.values())
            for right_half in right_group
        }
        if (
            len(left_group) != len(right_group)
            or any(len(matches) != 1 for matches in candidate_pairs.values())
            or any(count != 1 for count in reverse_counts.values())
        ):
            continue  # no traversal-independent unique pairing
        for left_half, matches in candidate_pairs.items():
            pairs.append(((left, right), (left_half, matches[0]), left_half.edge))
    return pairs


def _assert_pairs_identically(graph: FaceGraph, a: FaceNode, b: FaceNode, where: str) -> int:
    """Every field of every issued occurrence, in order, against the scan's answer.

    ``SharedEdgeOccurrenceRef`` and ``EdgeOccurrenceRef`` are ``eq=False``, so ``==`` on the
    endpoint and half tuples is identity -- exactly the comparison these references are for.
    The edge is checked with ``is`` on purpose: ``Edge.__eq__`` is ``IsSame``, which would pass
    for the *other* face's reading of the same edge, and the method promises the left half's.

    *where* names the part, because the sweep below runs this over 13,233 pairs and a report
    saying only that two lists of opaque references differ would not say where to look.
    """

    context = f"{where}: nodes {a.index} and {b.index}"
    expected = _scanned_pairing(graph, a, b)
    issued = graph.shared_occurrences(a, b)
    assert [(item.endpoints, item.halves) for item in issued] == [
        (endpoints, halves) for endpoints, halves, _ in expected
    ], f"{context} paired {len(issued)} occurrences, against the scan's {len(expected)}"
    assert [id(item.edge) for item in issued] == [id(edge) for _, _, edge in expected], (
        f"{context} paired the scan's occurrences but carried a different Edge wrapper; the "
        "occurrence must carry the left half's own reading, not the right half's"
    )
    return len(issued)


@pytest.fixture(scope="module")
def sentinel_graph() -> FaceGraph:
    return FaceGraph(import_step_geometry(_SENTINEL_PART))


@pytest.mark.slow
@pytest.mark.skipif(not CORPUS_PARTS, reason="the vendored corpus is not present")
def test_every_adjacent_pair_of_every_corpus_part_pairs_identically() -> None:
    """The equivalence that matters, over the whole vendored corpus.

    All 89 parts, every adjacent pair of every graph: 13,233 pairs and 13,856 issued
    occurrences -- so there is no reason to sample.

    The corpus exercises the grouping and the pairing, but every one of its shared groups holds
    exactly one half on each side; nothing here ever rejects a pair or meets a seam.
    ``test_a_seam_edge_groups_with_its_own_reversed_reading`` covers the multi-member group the
    corpus lacks, and the uniqueness rule below it is unchanged code either way.

    Slow, and marked here rather than in ``tests/conftest.py``'s ``SLOW_MODULES``, because this
    is the one test in the module that belongs in the slow tier and the reason is about the test
    and not the file: at roughly six seconds it is the whole module's cost, and its value is
    highest before merge and decays afterwards, when all it re-proves is an equivalence nothing
    is changing. Everything else here -- the seam grouping, the traversal-order agreement, the
    operation-count sentinel -- is what fails when this work is undone, is cheap, and stays in
    the fast tier. It builds its own graphs and shares no module-scoped geometry with them, so
    splitting the module across tiers rebuilds nothing.
    """

    seen_pairs = 0
    seen_occurrences = 0
    for path in CORPUS_PARTS:
        graph = FaceGraph(import_step_geometry(path))
        for node in graph.nodes:
            for neighbour in graph.neighbours(node):
                if graph._at(neighbour) <= graph._at(node):
                    continue  # the pair is symmetric; the second call only reads the cache
                seen_occurrences += _assert_pairs_identically(graph, node, neighbour, path.name)
                seen_pairs += 1
    assert (seen_pairs, seen_occurrences) == (13233, 13856), (
        f"the sweep visited {seen_pairs} adjacent pairs and paired {seen_occurrences} "
        "occurrences; 13233 and 13856 are what the vendored corpus holds. A drop means the "
        "sweep stopped covering what it claims to, not that the pairing changed -- that would "
        "have failed inside _assert_pairs_identically first."
    )


@pytest.mark.parametrize(
    "build",
    [
        pytest.param(lambda: Box(10, 10, 10), id="box"),
        pytest.param(lambda: Cylinder(5, 10), id="cylinder"),
    ],
)
def test_built_geometry_pairs_identically_in_both_directions(build: _Build) -> None:
    """Asking the other way round is the same answer, cached, and still the scan's answer."""

    graph = FaceGraph(build())
    for node in graph.nodes:
        for neighbour in graph.neighbours(node):
            issued = graph.shared_occurrences(node, neighbour)
            assert issued is graph.shared_occurrences(neighbour, node)
    fresh = FaceGraph(build())
    seen = 0
    for node in fresh.nodes:
        for neighbour in fresh.neighbours(node):
            seen += _assert_pairs_identically(fresh, node, neighbour, "built geometry")
    assert seen, "the fixture would be vacuous with nothing shared"


@pytest.mark.parametrize(
    ("build", "expected_seams"),
    [
        pytest.param(lambda: Cylinder(5, 10), 1, id="cylinder"),
        pytest.param(lambda: Sphere(5), 1, id="sphere"),
        pytest.param(lambda: Torus(10, 3), 2, id="torus"),
    ],
)
def test_a_seam_edge_groups_with_its_own_reversed_reading(
    build: _Build, expected_seams: int
) -> None:
    """The multi-member group the corpus never produces, and why the key finds it.

    A closed surface's face walks its seam twice, once forward and once reversed, and the two
    readings are distinct ``Edge`` wrappers. The scan matched them with ``IsSame``; the dict
    matches them because ``hash`` is the ``TopoDS_Shape``'s, which ignores orientation for
    exactly that reason. A key that carried orientation would split every seam in two.
    """

    graph = FaceGraph(build())
    seams = 0
    for node in graph.nodes:
        halves = graph._face_edge_occurrences(node)
        grouped = list(graph._halves_by_edge(halves).values())
        assert grouped == _scanned_groups(halves), f"node {node.index} grouped unlike the scan"
        for group in grouped:
            if len(group) > 1:
                seams += 1
                assert {half.reversed for half in group} == {False, True}, (
                    f"node {node.index} grouped {len(group)} halves that are not a seam's two "
                    "opposite readings, so this fixture is not testing what it says it is"
                )
    assert seams == expected_seams, (
        f"grouped {seams} multi-member groups, not {expected_seams}; this fixture exists to "
        "produce the seam the corpus never does, so a zero here means it stopped doing so"
    )


@pytest.mark.skipif(not CORPUS_PARTS, reason="the vendored corpus is not present")
def test_grouping_agrees_with_the_scan_on_every_face_of_a_real_part(sentinel_graph) -> None:
    """Group order, and each group's member order, are the traversal's -- on imported geometry.

    Imported geometry is where the two could have come apart. Insertion order is what makes a
    dict able to stand in for a loop that pops the first pending half: groups come out in the
    order of their first half, and members in traversal order.
    """

    widest = 0
    for node in sentinel_graph.nodes:
        halves = sentinel_graph._face_edge_occurrences(node)
        grouped = list(sentinel_graph._halves_by_edge(halves).values())
        assert grouped == _scanned_groups(halves), (
            f"node {node.index} of {_SENTINEL_PART.name} grouped its {len(halves)} halves "
            "unlike the scan: either the order or the membership of a group moved"
        )
        widest = max(widest, len(halves))
    assert (len(sentinel_graph.nodes), widest) == (664, 120), (
        f"{_SENTINEL_PART.name} read as {len(sentinel_graph.nodes)} faces with at most {widest} "
        "halves on one of them; 664 and 120 are what it holds, and 120 is the face whose L² "
        "scan the analysis named. A change here is the fixture moving, not the grouping"
    )


#: One census of the sentinel part, before and after the grouping pass.
#:
#: Both numbers are ``TopoDS_Shape.IsSame`` calls made underneath ``shared_occurrences`` during
#: one ``feature_census`` of ``nist_ctc_02``, over an unchanged 3,138 calls to the method. What
#: is left is one confirmation per dict lookup whose key is a different wrapper for the same
#: edge -- which is the ``IsSame`` the scan was really asking, asked once instead of ``L`` times.
#: Across the whole census the part's ``IsSame`` count fell from 1,211,185 to 65,280.
_ISSAME_BEFORE = 1147399
_ISSAME = 1494
_SHARED_OCCURRENCE_CALLS = 3138


def test_one_census_pairs_occurrences_without_rescanning(monkeypatch) -> None:
    """An operation-count sentinel, not a wall-clock one.

    A grouping that goes back to scanning leaves every answer correct and only makes the run
    slower -- counting the shape comparisons is the only way that shows up in CI. The count is
    taken underneath the method rather than over the whole census so that it moves when this
    code moves and not when some other recogniser's does.
    """

    comparisons = [0]
    inside = [0]
    calls = [0]
    original_is_same = TopoDS_Shape.IsSame
    original_occurrences = FaceGraph.shared_occurrences

    def counted_is_same(self, other):
        if inside[0]:
            comparisons[0] += 1
        return original_is_same(self, other)

    def counted_occurrences(self, a, b):
        calls[0] += 1
        inside[0] += 1
        try:
            return original_occurrences(self, a, b)
        finally:
            inside[0] -= 1

    monkeypatch.setattr(TopoDS_Shape, "IsSame", counted_is_same)
    monkeypatch.setattr(FaceGraph, "shared_occurrences", counted_occurrences)

    feature_census(import_step_geometry(_SENTINEL_PART))

    assert calls[0] == _SHARED_OCCURRENCE_CALLS, (
        f"a census of {_SENTINEL_PART.name} asked shared_occurrences {calls[0]} questions, not "
        f"{_SHARED_OCCURRENCE_CALLS}. This is the denominator of the comparison count below, "
        "not the thing this PR changed: one recogniser asking the graph one more question moves "
        "it without any regression here. Update both numbers together, and only after checking "
        "that the per-call ratio has not risen."
    )
    assert comparisons[0] == _ISSAME, (
        f"a census of {_SENTINEL_PART.name} made {comparisons[0]} TopoDS_Shape.IsSame calls "
        f"inside {calls[0]} shared_occurrences calls; {_ISSAME} is the count once each face's "
        f"halves are grouped in one pass, and {_ISSAME_BEFORE} was the count when the grouping "
        "rescanned. A rise means the pairing went back to scanning; a fall means this sentinel "
        "needs updating."
    )
