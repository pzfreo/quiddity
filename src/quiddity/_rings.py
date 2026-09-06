# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Closed prismatic rings of planar walls: finding them, measuring them, and asking what caps them.

A **ring** is a cycle of planar faces whose spans along one principal axis all agree, each meeting
exactly two others -- the walls of a constant-cross-section void running along that axis. What
caps its ends decides which feature it is:

- **neither end** -- a passage, running clean through the material;
- **one end** -- a pocket with a floor;
- **both** -- an enclosed cavity, which no family here reports.

This lived inside `passages` while that was the only family walking rings, and moved when a
second wanted them. The distinction it turns on is one line of `recognise_passages` --
``continue  # a floor fills the ring: this is a pocket`` -- which is to say the passage family has
always found pockets and thrown them away.

Nothing here decides what a ring *is*. It reports the shape, the span and the caps; a recogniser
reads those and says whether it has a feature.
"""

from __future__ import annotations

from collections.abc import Iterator
from dataclasses import dataclass

from OCP.BRepClass3d import BRepClass3d_SolidClassifier
from OCP.gp import gp_Pnt
from OCP.TopAbs import TopAbs_OUT

from quiddity._adjacency import FaceGraph, FaceNode, connected_components
from quiddity._geometry import AXIS_ZERO_COS
from quiddity._typing import Part

#: Spans agree to this to count as one ring. A coordinate comparison, not a size, so absolute
#: and dimensionless of the part (ADR 0008).
SPAN_EPS = 1e-6


@dataclass(frozen=True)
class Ring:
    """One closed ring, with everything a recogniser needs to judge it.

    ``nodes`` walks the cycle; ``section`` is its cross-section corners in the two axes other
    than ``axis``; ``low``/``high`` bound its span along ``axis``; ``cap_nodes`` retains the
    exact graph identities whose existing end proof says they fill each end. ``caps`` is the
    compatibility boolean view of those identities.
    """

    nodes: tuple[FaceNode, ...]
    section: tuple[tuple[float, float], ...]
    axis: int
    low: float
    high: float
    cap_nodes: tuple[frozenset[FaceNode], frozenset[FaceNode]]

    @property
    def caps(self) -> tuple[bool, bool]:
        return bool(self.cap_nodes[0]), bool(self.cap_nodes[1])


def rings(part: Part, graph: FaceGraph) -> Iterator[Ring]:
    """Every closed prismatic ring bounding a void, on any principal axis.

    Yields rings whatever caps them; a caller filters on :attr:`Ring.caps`. Rings that bound
    *material* rather than a void are not yielded at all -- the same walls bound a prism seen
    from outside, and that is `polygonal_bosses`' feature rather than a void.
    """

    # The walk starts from the graph's own nodes, so an unpaired graph is never asked to
    # resolve one of *this* part's faces -- it would answer about the wrong solid rather than
    # refuse, which is the one failure `require_node` exists to prevent. Every family that
    # resolves faces as it goes is refused for free; a family that walks the graph has to ask.
    # A dict lookup per face against an index already built.
    for face in part.faces():
        graph.require_node(face)

    planar = [node for node in graph.nodes if graph.is_planar(node)]
    normal = {node: graph.normal(node) for node in planar}
    for axis in (0, 1, 2):
        walls = [
            node
            for node in planar
            if normal[node] is not None and abs(normal[node][axis]) <= AXIS_ZERO_COS  # type: ignore[index]
        ]
        adjacent = {node: set(graph.neighbours(node)) for node in walls}

        def shares_a_span(
            a: FaceNode, b: FaceNode, axis: int = axis, adjacent: dict = adjacent
        ) -> bool:
            return (
                b in adjacent[a]
                and abs(graph.bounds(a)[axis][0] - graph.bounds(b)[axis][0]) <= SPAN_EPS
                and abs(graph.bounds(a)[axis][1] - graph.bounds(b)[axis][1]) <= SPAN_EPS
            )

        for ring in connected_components(walls, shares_a_span):
            members = set(ring)
            if len(ring) < 3 or any(len(adjacent[node] & members) != 2 for node in ring):
                continue  # a ring closes; a chain of walls does not
            section = _cross_section(graph, ring, members, axis)
            if section is None:
                continue  # the walls do not meet in a simple prismatic polygon
            spans = [graph.bounds(node)[axis] for node in ring]
            low, high = min(a for a, _ in spans), max(b for _, b in spans)
            if not _is_void(part, section, axis, low, high):
                continue
            yield Ring(
                ring,
                section,
                axis,
                low,
                high,
                _capped_ends(graph, ring, members, axis, low, high),
            )


def _cross_section(
    graph: FaceGraph, ring: tuple[FaceNode, ...], members: set[FaceNode], axis: int
) -> tuple[tuple[float, float], ...] | None:
    """The ring's corners, walked around it, or None when it is not a prismatic polygon.

    Each corner is where two consecutive walls meet, so it is read from the edge they share
    rather than from an average of anything. That edge must be a single one parallel to the run
    axis: two walls meeting along two edges, or along an edge that is not straight down the
    passage, are not the prismatic ring this family recognises, and returning None is how they
    are declined rather than silently mis-measured.
    """

    others = [a for a in (0, 1, 2) if a != axis]
    order = [ring[0]]
    seen = {ring[0]}
    while len(order) < len(ring):
        # Every member has exactly two in-ring neighbours and the component is connected, so
        # the members form one cycle and there is always exactly one unvisited step. A bare
        # `next` rather than a decline: were that invariant ever to break, raising is the
        # honest answer, and a `return None` here would be an untestable branch pretending to
        # handle a case its caller has already ruled out.
        step = next(n for n in graph.neighbours(order[-1]) if n in members and n not in seen)
        seen.add(step)
        order.append(step)

    corners: list[tuple[float, float]] = []
    for at, node in enumerate(order):
        # Two planes both parallel to the run axis meet in one line parallel to it, so every
        # edge two consecutive walls share lies on that line and the whole junction is a single
        # point across the section -- however many segments the kernel split it into, and
        # without needing to check, because the wall filter above is what guarantees it. What
        # is *not* guaranteed is that the corners then form a polygon, and `_canonical` is
        # where that is decided.
        boxes = [
            edge.bounding_box() for edge in graph.shared_edges(node, order[(at + 1) % len(order)])
        ]
        across, along = (
            0.5
            * (
                min(getattr(box.min, "XYZ"[a]) for box in boxes)
                + max(getattr(box.max, "XYZ"[a]) for box in boxes)
            )
            for a in others
        )
        corners.append((across, along))
    return _canonical(corners)


def _canonical(corners: list[tuple[float, float]]) -> tuple[tuple[float, float], ...] | None:
    """One walk per shape, whatever order the kernel handed the faces over in.

    Anticlockwise from the lexicographically smallest corner. Without this the record would
    carry the traversal, and `tests/golden/traversal_order` exists because that is exactly the
    kind of thing that leaks into a record unnoticed.
    """

    count = len(corners)
    if len(set(corners)) != count:
        return None  # two walls meeting at one point is not a simple polygon
    twice_area = sum(
        corners[at][0] * corners[(at + 1) % count][1]
        - corners[(at + 1) % count][0] * corners[at][1]
        for at in range(count)
    )
    if abs(twice_area) <= SPAN_EPS:
        return None  # degenerate: the corners are collinear
    if twice_area < 0:
        corners = corners[::-1]
    start = min(range(count), key=lambda at: corners[at])
    return tuple(corners[start:] + corners[:start])


def _centroid(section: tuple[tuple[float, float], ...]) -> tuple[float, float]:
    """The polygon's area centroid, which is what ``at`` means.

    Not the average of the wall-face centres the first draft used: that is the centroid of the
    *walls*, which for an irregular polygon is a different point, and one nothing downstream
    could reproduce from the record.
    """

    count = len(section)
    twice_area = 0.0
    across = 0.0
    along = 0.0
    for at in range(count):
        u0, v0 = section[at]
        u1, v1 = section[(at + 1) % count]
        cross = u0 * v1 - u1 * v0
        twice_area += cross
        across += (u0 + u1) * cross
        along += (v0 + v1) * cross
    return (across / (3 * twice_area), along / (3 * twice_area))


def _interior_point(section: tuple[tuple[float, float], ...]) -> tuple[float, float]:
    """A point proved to lie inside the cross-section, for the material probe.

    The centroid will not do. It is inside a convex polygon and can be outside a concave one, and
    an L-shaped or star-shaped passage is exactly the case where "is the material inside this
    ring" must still be answered correctly. Probing a point that is not in the cross-section can
    read an unrelated cavity, the material beside the void, or nothing at all.

    The construction is the standard one for a simple polygon: the lowest corner is convex, so
    the triangle it makes with its two neighbours lies wholly inside unless another corner
    intrudes into it, and that triangle's centroid is then interior. When one does intrude, the
    segment from the lowest corner to the intruder farthest from the chord is a diagonal, and
    its midpoint is interior.

    The centroid of the ear and not the midpoint of the chord: for a triangular passage the
    chord *is* the opposite edge, so its midpoint lies on the boundary and the classifier
    answers ``ON`` -- which is how this was caught, the three-sided fixture going missing.
    """

    count = len(section)
    at = min(range(count), key=lambda k: (section[k][1], section[k][0]))
    before, corner, after = section[at - 1], section[at], section[(at + 1) % count]
    intruders = [
        point
        for k, point in enumerate(section)
        if k not in {(at - 1) % count, at, (at + 1) % count}
        and _within(point, before, corner, after)
    ]
    if not intruders:
        return (
            (before[0] + corner[0] + after[0]) / 3,
            (before[1] + corner[1] + after[1]) / 3,
        )
    deepest = max(intruders, key=lambda point: abs(_turn(before, after, point)))
    return (0.5 * (corner[0] + deepest[0]), 0.5 * (corner[1] + deepest[1]))


def _turn(a: tuple[float, float], b: tuple[float, float], c: tuple[float, float]) -> float:
    return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])


def _within(
    point: tuple[float, float],
    a: tuple[float, float],
    b: tuple[float, float],
    c: tuple[float, float],
) -> bool:
    """Is *point* inside or on triangle *a*, *b*, *c*?"""

    turns = (_turn(a, b, point), _turn(b, c, point), _turn(c, a, point))
    return all(turn >= 0 for turn in turns) or all(turn <= 0 for turn in turns)


def _is_void(
    part: Part, section: tuple[tuple[float, float], ...], axis: int, low: float, high: float
) -> bool:
    """Is the ring's interior empty, rather than a prism of material?

    Only ``OUT`` is a passage. The first draft rejected only ``IN``, which let ``ON`` and
    ``UNKNOWN`` through as though they were evidence of a void: ``ON`` says the probe landed on
    a face and ``UNKNOWN`` says the classifier could not decide, and neither is a reason to
    report a through feature. With a point proved interior, both mean something is wrong with
    the ring rather than with the probe, so they fail closed.
    """

    others = [a for a in (0, 1, 2) if a != axis]
    point = [0.0, 0.0, 0.0]
    point[axis] = 0.5 * (low + high)
    point[others[0]], point[others[1]] = _interior_point(section)
    probe = BRepClass3d_SolidClassifier(part.wrapped)
    probe.Perform(gp_Pnt(*point), SPAN_EPS)
    return bool(probe.State() == TopAbs_OUT)


def _capped_ends(
    graph: FaceGraph,
    ring: tuple[FaceNode, ...],
    members: set[FaceNode],
    axis: int,
    low: float,
    high: float,
) -> tuple[frozenset[FaceNode], frozenset[FaceNode]]:
    """Exact neighbouring faces that close each end of *ring*.

    Reported per end rather than as one bool because the count is what names the feature: neither
    end is a passage, one is a pocket with a floor, both is an enclosed cavity. A single "is it
    capped" answer served the family that only needed to reject any cap, and hid the distinction
    the second family turns on.


    Not merely "is there a perpendicular neighbour at the end" -- at a passage mouth the part's
    own outer face is perpendicular and edge-adjacent, and testing only for its presence
    rejects every passage there is. A floor *fills* the ring's cross-section; an end face
    extends past it, because the ring is a hole punched through that face.
    """

    others = [a for a in (0, 1, 2) if a != axis]
    ring_low = [min(graph.bounds(node)[a][0] for node in ring) for a in others]
    ring_high = [max(graph.bounds(node)[a][1] for node in ring) for a in others]
    caps: tuple[set[FaceNode], set[FaceNode]] = (set(), set())
    for node in ring:
        for other in graph.neighbours(node):
            if other in members:
                continue
            # Any neighbour, not only a planar axis-aligned one. Requiring that let an
            # ordinary filleted or chamfered pocket floor read as a passage: breaking the
            # bottom edge replaces the flat floor's contact with a blend face, and the only
            # cap candidate disappeared. A blend at the bottom of a blind void still closes
            # it, so what matters is whether something sits across the end of the span
            # inside the ring, not what surface type it happens to be.
            # Every ring member's span *is* `low`..`high` -- that is what put them in one ring
            # -- and a neighbour shares an edge with one, so its own span always overlaps.
            # There is nothing to reject on that count, only on where within the span it sits.
            end = graph.bounds(other)
            near_low = abs(end[axis][0] - low) <= SPAN_EPS or abs(end[axis][1] - low) <= SPAN_EPS
            near_high = abs(end[axis][0] - high) <= SPAN_EPS or abs(end[axis][1] - high) <= SPAN_EPS
            if not (near_low or near_high):
                continue
            if all(
                end[a][0] >= ring_low[k] - SPAN_EPS and end[a][1] <= ring_high[k] + SPAN_EPS
                for k, a in enumerate(others)
            ):
                if near_low:
                    caps[0].add(other)
                if near_high:
                    caps[1].add(other)
    return frozenset(caps[0]), frozenset(caps[1])
