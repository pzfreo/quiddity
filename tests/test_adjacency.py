# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""One answer to "which faces meet along this edge", shared by every recogniser.

Five modules used to answer it separately: an edge→faces dict in ``_hole_features``, a second
one inline in ``fillets``, memoised pairwise closures in ``polygonal_bosses``, and raw
``IsSame`` sweeps in ``chamfers`` and ``flats``. The duplicated *source* was about twenty-five
lines. The risk was never the line count — it was that the dict form keys on build123d shape
equality while the sweeps compared with ``TopoDS_Shape.IsSame``, and **nothing tested that
those two notions of face identity agree**.

So the first test here is the one that made the consolidation safe to do at all: it proves the
two predicates induce the same partition of the edges *and* the faces of every pinned fixture.
The rest pin the behaviour the five call sites relied on.
"""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest
from build123d import Axis, Box, Cylinder, Pos, Rot, SortBy, chamfer, fillet

from quiddity import (
    recognise_chamfers,
    recognise_fillets,
    recognise_holes,
    recognise_slots,
)
from quiddity._adjacency import (
    FaceEdges,
    axis_aligned_axis,
    connected_components,
    edge_face_map,
    nearest_axis_aligned_planes,
    neighbours,
)

GOLDEN_ROOT = Path(__file__).parent / "golden"


def _partition_by_equality(shapes) -> list[list[int]]:
    groups: dict = {}
    for index, shape in enumerate(shapes):
        groups.setdefault(shape, []).append(index)
    return sorted(sorted(members) for members in groups.values())


def _partition_by_is_same(shapes) -> list[list[int]]:
    representatives: list = []
    groups: dict[int, list[int]] = {}
    for index, shape in enumerate(shapes):
        for rep_index, rep in enumerate(representatives):
            if rep.IsSame(shape.wrapped):
                groups[rep_index].append(index)
                break
        else:
            representatives.append(shape.wrapped)
            groups[len(representatives) - 1] = [index]
    return sorted(sorted(members) for members in groups.values())


def test_shape_equality_and_is_same_agree_across_the_whole_corpus():
    """The premise the module rests on, measured rather than assumed.

    build123d shape equality is ``TShape`` + ``Location`` and orientation-insensitive — the
    same predicate ``IsSame`` implements. If that ever stopped being true, keying an
    edge→faces dict would silently stop agreeing with the pairwise comparisons it replaced,
    and face adjacency would change for every recogniser at once with nothing to catch it.

    Checked on every pinned fixture rather than one shape, because the two predicates can
    only diverge on topology that shares a ``TShape`` — seams, split faces, repeated
    sub-shapes — which no single hand-written solid reliably produces.
    """

    checked = 0
    for path in sorted(GOLDEN_ROOT.glob("*/fixture.py")):
        spec = importlib.util.spec_from_file_location(f"fixture_{path.parent.name}", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        part = module.build_fixture()
        name = path.parent.name

        edges = [edge for face in part.faces() for edge in face.edges()]
        assert _partition_by_equality(edges) == _partition_by_is_same(edges), f"edges of {name}"

        faces = list(part.faces())
        assert _partition_by_equality(faces) == _partition_by_is_same(faces), f"faces of {name}"
        checked += 1

    assert checked == 30, "the corpus moved; this test must still sweep all of it"


def test_a_manifold_edge_maps_to_the_two_faces_that_meet_along_it():
    """A box has twelve edges, each shared by exactly two of its six faces."""

    edge_faces = edge_face_map(Box(10, 10, 10).faces())

    assert len(edge_faces) == 12
    assert {len(faces) for faces in edge_faces.values()} == {2}


def test_a_seam_edge_maps_to_a_single_face():
    """A closed cylindrical surface carries a seam belonging to one face only.

    ``edge_face_map`` must not promise two faces per edge. A caller that assumed it would
    misread a plain turned shaft, where the seam is an ordinary part of the topology rather
    than a defect.
    """

    edge_faces = edge_face_map(Cylinder(5, 20).faces())

    assert sorted({len(faces) for faces in edge_faces.values()}) == [1, 2]


def test_neighbours_excludes_the_face_itself_and_never_repeats_one():
    """A box face touches the four faces around it — each once, and not itself.

    Both properties are load-bearing. ``recognise_chamfers`` and ``recognise_fillets`` keep
    the nearest neighbour plane per axis, so a face yielded twice would be weighed twice,
    and the face itself appearing as its own neighbour is trivially the nearest of all.
    """

    part = Box(10, 10, 10)
    edge_faces = edge_face_map(part.faces())
    face = part.faces()[0]

    found = neighbours(face, edge_faces)

    assert len(found) == 4
    assert face not in found
    assert len(set(found)) == len(found)


def test_axis_aligned_axis_reads_the_axis_and_where_the_plane_sits():
    """Both halves matter: the axis says which wall this is, the coordinate says where.

    ``recognise_chamfers`` and ``recognise_fillets`` rebuild the virtual sharp corner a blend
    replaces by crossing two of these planes, so a right axis with a wrong coordinate would
    put the corner somewhere else entirely and silently flip the convex test.
    """

    read = {axis_aligned_axis(face.wrapped) for face in Box(10, 10, 10).faces()}

    assert read == {(0, -5.0), (0, 5.0), (1, -5.0), (1, 5.0), (2, -5.0), (2, 5.0)}


def test_axis_aligned_axis_rejects_an_oblique_plane_and_a_curved_face():
    """The two ways a face fails to be a wall — angled, or not a plane at all.

    A chamfer face is itself an oblique plane and a fillet face is a cylinder, so without
    both rejections a blend would be offered as its own neighbour's supporting wall.
    """

    oblique = Rot(0, 0, 30) * Box(10, 10, 10)
    sides = [f for f in oblique.faces() if abs(f.normal_at().Z) < 0.5]

    assert sides and all(axis_aligned_axis(f.wrapped) is None for f in sides)
    assert axis_aligned_axis(Cylinder(5, 20).faces().sort_by(SortBy.AREA)[-1].wrapped) is None


def test_nearest_axis_aligned_planes_breaks_a_tie_on_coordinate_not_arrival():
    """A cube's side face is equidistant from all four faces around it — a real tie.

    This is the order-dependence that was live in ``recognise_chamfers`` before the shared
    query: with a strict ``<`` the winner was whichever face the kernel happened to yield
    first, so the reconstructed corner — and with it the convex test — depended on traversal
    order. The compatibility behavior resolves to the lower coordinate.
    """

    part = Box(10, 10, 10)
    face = next(f for f in part.faces() if f.normal_at().X > 0.9)
    edge_faces = edge_face_map(part.faces())
    centre = {0: 5.0, 1: 0.0, 2: 0.0}

    # The tie is genuine, not an artefact of this cube's numbering: both candidate planes on
    # each axis sit the same distance away, so only the tie break decides.
    candidates = [axis_aligned_axis(n.wrapped) for n in neighbours(face, edge_faces)]
    assert sorted(c for c in candidates if c is not None) == [
        (1, -5.0),
        (1, 5.0),
        (2, -5.0),
        (2, 5.0),
    ]

    assert nearest_axis_aligned_planes(face, edge_faces, centre, exclude_axis=0) == {
        1: -5.0,
        2: -5.0,
    }


@pytest.mark.parametrize("face_axis", range(3))
def test_nearest_axis_aligned_planes_can_refuse_distinct_equidistant_planes(
    face_axis: int,
):
    part = Box(10, 10, 10)
    face = next(f for f in part.faces() if tuple(f.normal_at())[face_axis] > 0.9)
    edge_faces = edge_face_map(part.faces())
    centre = {axis: 5.0 if axis == face_axis else 0.0 for axis in range(3)}

    assert (
        nearest_axis_aligned_planes(
            face,
            edge_faces,
            centre,
            exclude_axis=face_axis,
            refuse_equidistant=True,
        )
        == {}
    )


def test_nearest_axis_aligned_planes_drops_the_axis_the_feature_runs_along():
    """A plane facing along the run axis is an end cap, not a wall the blend bridges.

    Excluding it is what stops a slot end cap or a part's outer face from standing in for a
    missing neighbour and letting the corner be rebuilt from a plane the blend never touches.
    """

    part = Box(10, 10, 10)
    face = next(f for f in part.faces() if f.normal_at().X > 0.9)
    edge_faces = edge_face_map(part.faces())
    centre = {0: 5.0, 1: 0.0, 2: 0.0}

    assert nearest_axis_aligned_planes(face, edge_faces, centre, exclude_axis=1) == {2: -5.0}


def test_nearest_axis_aligned_planes_keeps_the_closer_of_two_walls():
    """With no tie the nearest plane wins outright — the one forming this local corner.

    Driven by moving *centre* rather than by building a stepped solid: the distance ranking
    is what the query is contracted on, and a cube whose four walls are equidistant is the
    only shape that isolates it from the tie break tested above.
    """

    part = Box(10, 10, 10)
    face = next(f for f in part.faces() if f.normal_at().X > 0.9)
    edge_faces = edge_face_map(part.faces())

    # Bias the centre toward +Y so the +5 wall is unambiguously nearer than the -5 one.
    biased = nearest_axis_aligned_planes(face, edge_faces, {0: 5.0, 1: 4.0, 2: 0.0}, exclude_axis=0)

    assert biased[1] == 5.0
    assert biased[2] == -5.0  # still tied on Z, so compatibility chooses the lower plane


def test_nearest_axis_aligned_planes_tie_boundary_is_scale_aware_and_fail_closed():
    part = Box(10, 10, 10)
    face = next(f for f in part.faces() if f.normal_at().X > 0.9)
    edge_faces = edge_face_map(part.faces())

    within_noise = nearest_axis_aligned_planes(
        face,
        edge_faces,
        {0: 5.0, 1: 4e-7, 2: 0.0},
        exclude_axis=0,
        refuse_equidistant=True,
    )
    outside_noise = nearest_axis_aligned_planes(
        face,
        edge_faces,
        {0: 5.0, 1: 1e-5, 2: 0.0},
        exclude_axis=0,
        refuse_equidistant=True,
    )
    outside_noise_reversed = nearest_axis_aligned_planes(
        face,
        edge_faces,
        {0: 5.0, 1: -1e-5, 2: 0.0},
        exclude_axis=0,
        refuse_equidistant=True,
    )

    assert 1 not in within_noise
    assert outside_noise[1] == 5.0
    assert outside_noise_reversed[1] == -5.0


def test_face_edges_reuses_one_list_across_separate_face_wrappers():
    """The property the whole memo rests on, and the one that could silently fail.

    Two ``part.faces()`` calls yield *different Python objects* for the same face. If the
    memo missed on those, it would never share anything between recognisers — which is the
    only place it pays — while still costing a dict insert per face, so it would be a pure
    loss that no result-based test could detect. Keying on build123d shape equality is what
    makes the second wrapper hit; the corpus-wide equality/``IsSame`` test above is the proof
    that equality means ``IsSame``.
    """

    part = Box(10, 10, 10)
    first, second = part.faces()[0], part.faces()[0]
    memo = FaceEdges()

    assert first is not second, "the premise: a fresh walk yields fresh wrappers"
    assert memo.of(first) is memo.of(second)
    assert len(memo.of(first)) == 4


def test_face_edges_returns_what_the_kernel_returns():
    """Memoising must not change the answer, only how often it is computed."""

    face = Box(10, 10, 10).faces()[0]

    assert list(FaceEdges().of(face)) == list(face.edges())


def test_a_shared_memo_does_not_change_the_adjacency_map_or_neighbours():
    """The map and its neighbour queries are identical whether or not a memo is threaded.

    ``edge_face_map`` builds a private memo when none is passed, so these two paths are
    different code; this pins that the ``face_edges=`` parameter is an optimisation and
    never a behaviour switch.
    """

    part = Box(10, 10, 10)
    memo = FaceEdges()

    plain = edge_face_map(part.faces())
    shared = edge_face_map(part.faces(), face_edges=memo)

    assert plain == shared
    for face in part.faces():
        assert neighbours(face, plain) == neighbours(face, shared, face_edges=memo)


def test_sharing_one_memo_across_recognisers_leaves_every_result_unchanged():
    """The contract the census depends on, checked on a part that exercises the sharers.

    The memo is threaded through recognisers that each walk the same faces, so a stale or
    mis-keyed entry would show up as a changed feature count rather than as an exception.
    Run against a part carrying a hole, a slot, a chamfer and a fillet so the blend
    recognisers, the hole recogniser and the recess core all consult the same memo — and
    assert each one actually finds something first, because comparing two empty lists would
    pass no matter how badly the memo behaved.
    """

    part = Box(60, 40, 12) - Pos(-18, 0, 0) * Cylinder(4, 12) - Pos(15, 0, 0) * Box(24, 8, 12)
    part = chamfer(part.edges().filter_by(Axis.Z).group_by(Axis.X)[0], 1.5)
    part = fillet(part.edges().filter_by(Axis.Z).group_by(Axis.X)[-1], 2.0)

    memo = FaceEdges()
    for recognise in (recognise_chamfers, recognise_fillets, recognise_holes, recognise_slots):
        plain = recognise(part)
        assert plain, f"{recognise.__name__} found nothing; this part cannot pin the memo"
        assert plain == recognise(part, face_edges=memo)


def test_neighbours_is_empty_when_the_map_does_not_cover_the_face():
    """A face absent from the map has no neighbours rather than raising.

    The recognisers all build the map from the same part they query, so this is a contract
    for the helper rather than a path they reach — but it is the difference between a
    mis-paired map returning nothing and it raising ``KeyError`` from inside a recogniser.
    """

    assert neighbours(Box(10, 10, 10).faces()[0], {}) == []


def test_connected_components_groups_transitively_not_pairwise():
    """Items reachable through a chain land in one component, not several.

    The property the walk exists for: `polygonal_bosses` groups a hexagon's six side faces
    into one ring although no face touches more than two others.
    """

    chain = connected_components([0, 1, 2, 3, 4], lambda i, j: abs(i - j) == 1)

    assert [sorted(part) for part in chain] == [[0, 1, 2, 3, 4]]


def test_connected_components_keeps_unrelated_items_apart():
    """Two rings on one part stay two rings, and a lone face stays alone."""

    parts = connected_components([0, 1, 10, 11, 99], lambda i, j: abs(i - j) == 1)

    assert sorted(sorted(p) for p in parts) == [[0, 1], [10, 11], [99]]
    assert connected_components([], lambda i, j: True) == []
    assert connected_components([7], lambda i, j: True) == [(7,)]


def test_connected_components_needs_a_symmetric_predicate():
    """The precondition the docstring states, pinned rather than trusted.

    The walk only ever asks ``joined(current, other)`` with *current* already inside the
    component, so an asymmetric predicate makes the answer depend on set iteration order. The
    same relation written both ways must therefore agree; a caller whose predicate is not
    symmetric gets an answer that is not a function of its input.
    """

    forward = connected_components([0, 1, 2, 3, 4], lambda i, j: j == i + 1)
    backward = connected_components([0, 1, 2, 3, 4], lambda i, j: i == j + 1)

    assert [sorted(p) for p in forward] == [[0, 1, 2, 3, 4]]
    assert sorted(sorted(p) for p in backward) == [[0], [1], [2], [3], [4]]
    assert forward != backward, (
        "an asymmetric predicate gives two answers for one relation, which is why "
        "connected_components requires a symmetric one"
    )


def test_connected_components_matches_what_polygonal_bosses_used_to_do():
    """The lift is behaviour-neutral on shapes the corpus does not contain.

    Byte-identical goldens prove the grouping was preserved for the fixtures that exist. They
    say nothing about an empty input, a fully connected set, or singletons, so those are
    compared against the walk this replaced.
    """

    def previous(vertical, joined):
        rings = []
        unseen = set(vertical)
        while unseen:
            connected = {unseen.pop()}
            frontier = list(connected)
            while frontier:
                current = frontier.pop()
                attached = {o for o in unseen if joined(current, o)}
                unseen -= attached
                connected |= attached
                frontier.extend(attached)
            rings.append(tuple(connected))
        return rings

    for items, predicate in (
        ([], lambda i, j: True),
        ([3], lambda i, j: True),
        (list(range(8)), lambda i, j: True),
        (list(range(8)), lambda i, j: False),
        (list(range(12)), lambda i, j: abs(i - j) == 1),
        (list(range(12)), lambda i, j: i % 3 == j % 3),
    ):
        assert connected_components(items, predicate) == previous(items, predicate)
