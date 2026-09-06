# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Edge-open prismatic recess records and recognition."""

from __future__ import annotations

import math
from dataclasses import dataclass
from itertools import pairwise

from build123d import Solid, Vector

from quiddity._adjacency import FaceEdges, FaceGraph, FaceNode
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._geometry import AXIS_ZERO_COS
from quiddity._record import Record
from quiddity._rings import SPAN_EPS
from quiddity._typing import Part
from quiddity._volume_probe import intersection_volume

_AXES = "xyz"
_EPS = 1e-9


def _point(value: object, *, name: str) -> tuple[float, float]:
    if (
        not isinstance(value, tuple)
        or len(value) != 2
        or any(isinstance(item, bool) or not isinstance(item, int | float) for item in value)
    ):
        raise ValueError(f"{name} must be a pair of finite numbers")
    result = (float(value[0]), float(value[1]))
    if not all(math.isfinite(item) for item in result):
        raise ValueError(f"{name} must be a pair of finite numbers")
    if any(round(item, 4) != item for item in result):
        raise ValueError(f"{name} must serialize exactly at four decimal places")
    return tuple(0.0 if item == 0.0 else item for item in result)  # type: ignore[return-value]


def _turn(a: tuple[float, float], b: tuple[float, float], c: tuple[float, float]) -> float:
    return (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])


def _crosses(
    first: tuple[tuple[float, float], tuple[float, float]],
    second: tuple[tuple[float, float], tuple[float, float]],
) -> bool:
    a, b = first
    c, d = second
    if (
        max(a[0], b[0]) + _EPS < min(c[0], d[0])
        or max(c[0], d[0]) + _EPS < min(a[0], b[0])
        or max(a[1], b[1]) + _EPS < min(c[1], d[1])
        or max(c[1], d[1]) + _EPS < min(a[1], b[1])
    ):
        return False
    return _turn(a, b, c) * _turn(a, b, d) <= _EPS and _turn(c, d, a) * _turn(c, d, b) <= _EPS


def _validate_simple(chain: tuple[tuple[float, float], ...]) -> None:
    edges = tuple(pairwise(chain))
    for index, edge in enumerate(edges):
        for other_index in range(index + 1, len(edges)):
            adjacent = other_index == index + 1
            if not adjacent and _crosses(edge, edges[other_index]):
                raise ValueError("wall chain must be simple")


@dataclass(frozen=True, order=True, slots=True)
class OpenSectionOpening(Record):
    """The physical endpoints of an absent wall, with no implied joining segment."""

    start: tuple[float, float]
    end: tuple[float, float]

    def __post_init__(self) -> None:
        start = _point(self.start, name="opening start")
        end = _point(self.end, name="opening end")
        if math.dist(start, end) <= _EPS:
            raise ValueError("opening endpoints must be distinct")
        object.__setattr__(self, "start", start)
        object.__setattr__(self, "end", end)


@dataclass(frozen=True, order=True, slots=True)
class OpenPolygonalSection(Record):
    """A canonical physical wall chain plus its explicit non-wall opening side."""

    wall_chain: tuple[tuple[float, float], ...]
    opening: OpenSectionOpening

    def __post_init__(self) -> None:
        if not isinstance(self.wall_chain, tuple):
            raise ValueError("wall_chain must be a tuple")
        chain = tuple(_point(point, name="wall-chain point") for point in self.wall_chain)
        if len(chain) < 4:
            raise ValueError("an open polygonal section needs at least three wall segments")
        if any(
            math.dist(chain[index], chain[index + 1]) <= _EPS for index in range(len(chain) - 1)
        ):
            raise ValueError("adjacent wall-chain points must be distinct")
        _validate_simple(chain)
        if not isinstance(self.opening, OpenSectionOpening) or (
            self.opening.start,
            self.opening.end,
        ) != (chain[-1], chain[0]):
            raise ValueError("opening must run from the wall-chain end to its start")
        reverse = tuple(reversed(chain))
        if reverse < chain:
            raise ValueError("wall_chain must use its canonical direction")
        object.__setattr__(self, "wall_chain", chain)


@dataclass(frozen=True, order=True, slots=True)
class EdgeOpenPrismaticRecess(Record):
    """A blind prismatic recess with one physical side open to the stock exterior."""

    axis: str
    run_interval: tuple[float, float]
    open_sign: int
    section: OpenPolygonalSection

    def __post_init__(self) -> None:
        if self.axis not in _AXES:
            raise ValueError("axis must be 'x', 'y', or 'z'")
        if (
            not isinstance(self.run_interval, tuple)
            or len(self.run_interval) != 2
            or any(
                isinstance(item, bool) or not isinstance(item, int | float)
                for item in self.run_interval
            )
        ):
            raise ValueError("run_interval must be a pair of finite numbers")
        interval = (float(self.run_interval[0]), float(self.run_interval[1]))
        if not all(math.isfinite(item) for item in interval) or interval[1] - interval[0] <= _EPS:
            raise ValueError("run_interval must be finite and strictly increasing")
        if any(round(item, 3) != item for item in interval):
            raise ValueError("run_interval must serialize exactly at three decimal places")
        if self.open_sign not in (-1, 1):
            raise ValueError("open_sign must be -1 or 1")
        if not isinstance(self.section, OpenPolygonalSection):
            raise ValueError("section must be an OpenPolygonalSection")
        object.__setattr__(self, "run_interval", interval)


def _principal_plane(graph: FaceGraph, node: FaceNode) -> tuple[int, float] | None:
    normal = graph.normal(node)
    if normal is None:
        return None
    axes = [axis for axis in range(3) if abs(normal[axis]) >= 1.0 - AXIS_ZERO_COS]
    if len(axes) != 1:
        return None
    axis = axes[0]
    low, high = graph.bounds(node)[axis]
    return (axis, 0.5 * (low + high)) if high - low <= SPAN_EPS else None


def _at_principal_plane(graph: FaceGraph, node: FaceNode, axis: int, at: float) -> bool:
    plane = _principal_plane(graph, node)
    return plane is not None and plane[0] == axis and abs(plane[1] - at) <= SPAN_EPS


def _shared_segment(
    graph: FaceGraph, floor: FaceNode, wall: FaceNode, axis: int
) -> tuple[tuple[float, float], tuple[float, float]] | None:
    others = [other for other in range(3) if other != axis]
    found = []
    for occurrence in graph.shared_occurrences(floor, wall):
        edge = occurrence.edge
        vertices = tuple(edge.vertices())
        if edge.geom_type.name != "LINE" or len(vertices) != 2:
            continue
        points = []
        for vertex in vertices:
            values = (float(vertex.X), float(vertex.Y), float(vertex.Z))
            points.append((values[others[0]], values[others[1]]))
        found.append(tuple(points))
    return found[0] if len(found) == 1 else None  # type: ignore[return-value]


def _ordered_open_chain(
    graph: FaceGraph, walls: tuple[FaceNode, ...]
) -> tuple[FaceNode, ...] | None:
    wall_set = set(walls)
    adjacent = {
        wall: tuple(other for other in graph.neighbours(wall) if other in wall_set)
        for wall in walls
    }
    ends = sorted((wall for wall in walls if len(adjacent[wall]) == 1), key=lambda node: node.index)
    if len(ends) != 2 or any(len(adjacent[wall]) not in (1, 2) for wall in walls):
        return None
    ordered = [ends[0]]
    while len(ordered) < len(walls):
        choices = [node for node in adjacent[ordered[-1]] if node not in ordered]
        if len(choices) != 1:
            return None
        ordered.append(choices[0])
    return tuple(ordered) if set(ordered) == wall_set else None


def _complete_wall_boundaries(
    graph: FaceGraph,
    floor: FaceNode,
    walls: tuple[FaceNode, ...],
    mouth: FaceNode,
    exterior: tuple[FaceNode, ...],
) -> bool:
    """Prove that no side wall contains a hole, branch, or unaccounted interruption."""

    exterior_set = set(exterior)
    for index, wall in enumerate(walls):
        if len(tuple(graph.face(wall).wires())) != 1:
            return False
        required = {floor, mouth}
        if index:
            required.add(walls[index - 1])
        if index + 1 < len(walls):
            required.add(walls[index + 1])
        neighbours = set(graph.neighbours(wall))
        if not required <= neighbours:
            return False
        # A neighbour is a topological occurrence, not merely a face identity.  A second
        # breakout can meet an endpoint wall again through the same exterior face; the set
        # above deliberately loses that information, so prove the expected single boundary
        # occurrence for every adjacent face before using it for membership checks.
        if any(len(graph.shared_edges(wall, node)) != 1 for node in neighbours):
            return False
        extra = neighbours - required
        if 0 < index < len(walls) - 1:
            if extra:
                return False
            continue
        if not extra or not all(graph.arc(wall, node) == "convex" for node in extra):
            return False
        pending = list(extra & exterior_set)
        reached = set(pending)
        while pending:
            current = pending.pop()
            for node in extra - reached:
                if node in graph.neighbours(current) and graph.arc(current, node) == "convex":
                    reached.add(node)
                    pending.append(node)
        if reached != extra:
            return False
    return True


def _section_from_walls(
    graph: FaceGraph, floor: FaceNode, walls: tuple[FaceNode, ...], axis: int
) -> OpenPolygonalSection | None:
    segments = tuple(_shared_segment(graph, floor, wall, axis) for wall in walls)
    if any(segment is None for segment in segments):
        return None
    physical = tuple(segment for segment in segments if segment is not None)
    joins: list[tuple[float, float]] = []
    for left, right in pairwise(physical):
        shared = [a for a in left for b in right if math.dist(a, b) <= SPAN_EPS]
        if len(shared) != 1:
            return None
        joins.append(shared[0])
    first = next((point for point in physical[0] if math.dist(point, joins[0]) > SPAN_EPS), None)
    last = next((point for point in physical[-1] if math.dist(point, joins[-1]) > SPAN_EPS), None)
    if first is None or last is None:
        return None
    chain = tuple((round(u, 4), round(v, 4)) for u, v in (first, *joins, last))
    if tuple(reversed(chain)) < chain:
        chain = tuple(reversed(chain))
    try:
        return OpenPolygonalSection(chain, OpenSectionOpening(chain[-1], chain[0]))
    except ValueError:
        return None


def _material_fraction(part: Part, probe: Solid) -> float:
    result = part.intersect(probe)
    if result is None:
        return 0.0
    volume = intersection_volume(result)
    return volume / float(probe.volume)


def _exact_floor_proof(
    part: Part, graph: FaceGraph, floor: FaceNode, axis: int, floor_at: float, mouth_at: float
) -> bool:
    distance = mouth_at - floor_at
    thickness = max(2e-5, abs(distance) * 1e-4)
    toward, behind = [0.0] * 3, [0.0] * 3
    toward[axis] = distance
    behind[axis] = -math.copysign(thickness, distance)
    try:
        cavity = Solid.extrude(graph.face(floor), Vector(*toward))
        backing = Solid.extrude(graph.face(floor), Vector(*behind))
        return (
            _material_fraction(part, cavity) <= 1e-9
            and _material_fraction(part, backing) >= 1 - 1e-9
        )
    except (AttributeError, RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return False


def recognise_edge_open_prismatic_recesses(
    part: Part,
    *,
    face_edges: FaceEdges | None = None,
    ledger: ClaimLedger | EvidenceWriter | None = None,
) -> list[EdgeOpenPrismaticRecess]:
    """Recognise proved one-side-open polygonal recesses with at least three physical walls."""
    graph = FaceGraph(part, face_edges=face_edges) if ledger is None else ledger.graph
    found: list[tuple[EdgeOpenPrismaticRecess, tuple[FaceNode, ...], FaceNode]] = []
    for floor in graph.nodes:
        plane = _principal_plane(graph, floor) if graph.is_planar(floor) else None
        if plane is None:
            continue
        axis, floor_at = plane
        walls = tuple(
            sorted(
                (
                    node
                    for node in graph.neighbours(floor)
                    if graph.is_planar(node)
                    and graph.arc(floor, node) == "concave"
                    and (normal := graph.normal(node)) is not None
                    and abs(normal[axis]) <= AXIS_ZERO_COS
                ),
                key=lambda node: node.index,
            )
        )
        if len(walls) < 3 or (ordered := _ordered_open_chain(graph, walls)) is None:
            continue
        exterior = tuple(node for node in graph.neighbours(floor) if node not in walls)
        if (
            len(tuple(graph.face(floor).wires())) != 1
            or not exterior
            or not all(graph.arc(floor, node) == "convex" for node in exterior)
        ):
            continue
        far = []
        for wall in ordered:
            low, high = graph.bounds(wall)[axis]
            if abs(low - floor_at) <= SPAN_EPS:
                far.append(high)
            elif abs(high - floor_at) <= SPAN_EPS:
                far.append(low)
            else:
                break
        else:
            if max(far) - min(far) > SPAN_EPS or abs(far[0] - floor_at) <= SPAN_EPS:
                continue
            mouth_at = sum(far) / len(far)
            mouth_context = set(graph.neighbours(ordered[0]))
            for wall in ordered[1:]:
                mouth_context &= set(graph.neighbours(wall))
            mouths = tuple(
                node
                for node in mouth_context
                if _at_principal_plane(graph, node, axis, mouth_at)
                and all(graph.arc(node, wall) in ("convex", "smooth") for wall in ordered)
            )
            if len(mouths) != 1:
                continue
            mouth = mouths[0]
            first_normal = graph.normal(ordered[0])
            last_normal = graph.normal(ordered[-1])
            if (
                first_normal is None
                or last_normal is None
                or math.isclose(
                    abs(
                        sum(
                            left * right
                            for left, right in zip(first_normal, last_normal, strict=True)
                        )
                    ),
                    1.0,
                    rel_tol=0.0,
                    abs_tol=1e-9,
                )
                or not _complete_wall_boundaries(graph, floor, ordered, mouth, exterior)
            ):
                continue
            owner = graph.common_valid_solid((*ordered, floor))
            section = _section_from_walls(graph, floor, ordered, axis)
            if (
                owner is None
                or section is None
                or not _exact_floor_proof(
                    graph.solid_shape(owner), graph, floor, axis, floor_at, mouth_at
                )
            ):
                continue
            low, high = sorted((floor_at, mouth_at))
            record = EdgeOpenPrismaticRecess(
                _AXES[axis],
                (round(low, 3), round(high, 3)),
                1 if mouth_at > floor_at else -1,
                section,
            )
            found.append((record, ordered, floor))
    found.sort(key=lambda item: item[0])
    if ledger is not None:
        writer = ledger.writer if isinstance(ledger, ClaimLedger) else ledger
        for record, walls, floor in found:
            writer.add_defining(
                record,
                walls,
                family=FamilyId.EDGE_OPEN_PRISMATIC_RECESSES,
                constituent=(*walls, floor),
            )
    return [record for record, _walls, _floor in found]
