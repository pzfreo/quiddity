# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Blind circular-ended recesses whose physical section has one interrupted end."""

from __future__ import annotations

import math
from dataclasses import dataclass
from itertools import pairwise

from build123d import GeomType, Solid, Vector

from quiddity._adjacency import FaceEdges, FaceGraph, FaceNode
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._geometry import AXIS_ZERO_COS
from quiddity._record import Record
from quiddity._rings import SPAN_EPS
from quiddity._typing import Part
from quiddity._volume_probe import intersection_volume

_AXES = "xyz"
_POINT_DIGITS = 4
_ANGLE_DIGITS = 7
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
    if any(round(item, _POINT_DIGITS) != item for item in result):
        raise ValueError(f"{name} must serialize exactly at four decimal places")
    return tuple(0.0 if item == 0.0 else item for item in result)  # type: ignore[return-value]


@dataclass(frozen=True, order=True, slots=True)
class OpenCircularSectionSegment(Record):
    """One physically present line or circular arc in an open section chain."""

    kind: str
    start: tuple[float, float]
    end: tuple[float, float]
    center: tuple[float, float] | None = None
    radius: float | None = None
    sweep: float | None = None

    def __post_init__(self) -> None:
        start = _point(self.start, name="segment start")
        end = _point(self.end, name="segment end")
        if math.dist(start, end) <= _EPS:
            raise ValueError("segment endpoints must be distinct")
        if self.kind == "line":
            if self.center is not None or self.radius is not None or self.sweep is not None:
                raise ValueError("a line segment cannot carry circular values")
        elif self.kind == "arc":
            center = _point(self.center, name="arc center")
            if (
                isinstance(self.radius, bool)
                or not isinstance(self.radius, int | float)
                or not math.isfinite(float(self.radius))
                or float(self.radius) <= 0
                or round(float(self.radius), _POINT_DIGITS) != float(self.radius)
            ):
                raise ValueError("arc radius must be positive and serialize at four decimals")
            if (
                isinstance(self.sweep, bool)
                or not isinstance(self.sweep, int | float)
                or not math.isfinite(float(self.sweep))
                or abs(float(self.sweep)) <= _EPS
                or abs(float(self.sweep)) > math.pi + 1e-6
                or round(float(self.sweep), _ANGLE_DIGITS) != float(self.sweep)
            ):
                raise ValueError("arc sweep must be a finite non-zero value no greater than pi")
            # Public rounding is independently bounded by the ADR-0008 reconstruction
            # allowance; it is not reused as a discovery tolerance.
            radial_tol = 0.002
            if any(
                abs(math.dist(point, center) - float(self.radius)) > radial_tol
                for point in (start, end)
            ):
                raise ValueError("arc endpoints must lie on its circle")
            start_vector = (start[0] - center[0], start[1] - center[1])
            cosine = math.cos(float(self.sweep))
            sine = math.sin(float(self.sweep))
            swept_end = (
                center[0] + start_vector[0] * cosine - start_vector[1] * sine,
                center[1] + start_vector[0] * sine + start_vector[1] * cosine,
            )
            if math.dist(swept_end, end) > radial_tol:
                raise ValueError("arc sweep must connect its start to its end")
            object.__setattr__(self, "center", center)
            object.__setattr__(self, "radius", float(self.radius))
            object.__setattr__(self, "sweep", float(self.sweep))
        else:
            raise ValueError("segment kind must be 'line' or 'arc'")
        object.__setattr__(self, "start", start)
        object.__setattr__(self, "end", end)

    def reversed(self) -> OpenCircularSectionSegment:
        return OpenCircularSectionSegment(
            self.kind,
            self.end,
            self.start,
            self.center,
            self.radius,
            None if self.sweep is None else -self.sweep,
        )


@dataclass(frozen=True, order=True, slots=True)
class OpenCircularSection(Record):
    """An alternating real wall chain and the explicit gap between its loose ends."""

    segments: tuple[OpenCircularSectionSegment, ...]
    opening: tuple[tuple[float, float], tuple[float, float]]

    def __post_init__(self) -> None:
        if (
            not isinstance(self.segments, tuple)
            or len(self.segments) != 4
            or not all(isinstance(item, OpenCircularSectionSegment) for item in self.segments)
        ):
            raise ValueError("an open circular section requires four physical segments")
        if tuple(item.kind for item in self.segments) not in (
            ("arc", "line", "arc", "line"),
            ("line", "arc", "line", "arc"),
        ):
            raise ValueError("open circular section segments must alternate arcs and lines")
        if any(left.end != right.start for left, right in pairwise(self.segments)):
            raise ValueError("open circular section segments must form one continuous chain")
        arcs = tuple(item for item in self.segments if item.kind == "arc")
        if arcs[0].radius != arcs[1].radius:
            raise ValueError("open circular section arcs must have one equal radius")
        if sum(abs(abs(item.sweep or 0.0) - math.pi) <= 1e-4 for item in arcs) != 1:
            raise ValueError("open circular section requires exactly one intact semicircle")
        opening = (
            _point(self.opening[0], name="opening start"),
            _point(self.opening[1], name="opening end"),
        )
        if opening != (self.segments[-1].end, self.segments[0].start):
            raise ValueError("opening must run from the chain end to its start")
        reversed_segments = tuple(item.reversed() for item in reversed(self.segments))
        if reversed_segments < self.segments:
            raise ValueError("segments must use their canonical direction")
        object.__setattr__(self, "opening", opening)


@dataclass(frozen=True, order=True, slots=True)
class EdgeOpenCircularPocket(Record):
    """A blind constant-depth recess with a truthful interrupted circular-end profile."""

    axis: str
    run_interval: tuple[float, float]
    open_sign: int
    section: OpenCircularSection

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
        interval = tuple(float(item) for item in self.run_interval)
        if not all(math.isfinite(item) for item in interval) or interval[1] - interval[0] <= _EPS:
            raise ValueError("run_interval must be finite and strictly increasing")
        if any(round(item, 3) != item for item in interval):
            raise ValueError("run_interval must serialize exactly at three decimal places")
        if self.open_sign not in (-1, 1):
            raise ValueError("open_sign must be -1 or 1")
        if not isinstance(self.section, OpenCircularSection):
            raise ValueError("section must be an OpenCircularSection")
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
    return (axis, (low + high) / 2) if high - low <= SPAN_EPS else None


def _ordered_chain(graph: FaceGraph, nodes: tuple[FaceNode, ...]) -> tuple[FaceNode, ...] | None:
    available = set(nodes)
    adjacent = {
        node: tuple(
            other
            for other in graph.neighbours(node)
            if other in available and graph.arc(node, other) == "smooth"
        )
        for node in nodes
    }
    ends = [node for node in nodes if len(adjacent[node]) == 1]
    if len(ends) != 2 or any(len(adjacent[node]) not in (1, 2) for node in nodes):
        return None
    ordered = [min(ends, key=lambda node: node.index)]
    while len(ordered) < len(nodes):
        choices = [node for node in adjacent[ordered[-1]] if node not in ordered]
        if len(choices) != 1:
            return None
        ordered.append(choices[0])
    return tuple(ordered) if set(ordered) == available else None


def _project(point, axis: int) -> tuple[float, float]:
    values = (float(point.X), float(point.Y), float(point.Z))
    others = [candidate for candidate in range(3) if candidate != axis]
    return values[others[0]], values[others[1]]


def _rounded_point(point: tuple[float, float]) -> tuple[float, float]:
    return round(point[0], _POINT_DIGITS), round(point[1], _POINT_DIGITS)


def _arc_sweep(edge, start: tuple[float, float], end: tuple[float, float], axis: int) -> float:
    center = _project(edge.arc_center, axis)
    middle = _project(edge.position_at(0.5), axis)
    first = math.atan2(start[1] - center[1], start[0] - center[0])
    last = math.atan2(end[1] - center[1], end[0] - center[0])
    mid = math.atan2(middle[1] - center[1], middle[0] - center[0])
    positive = (last - first) % (2 * math.pi)
    contains_mid = (mid - first) % (2 * math.pi) <= positive + 1e-9
    return positive if contains_mid else positive - 2 * math.pi


def _segment(
    graph: FaceGraph, floor: FaceNode, wall: FaceNode, axis: int
) -> OpenCircularSectionSegment | None:
    occurrences = graph.shared_occurrences(floor, wall)
    if len(occurrences) != 1:
        return None
    edge = occurrences[0].edge
    if edge.geom_type not in (GeomType.LINE, GeomType.CIRCLE):
        return None
    vertices = tuple(edge.vertices())
    if len(vertices) != 2:
        return None
    start = _project(edge.position_at(0.0), axis)
    end = _project(edge.position_at(1.0), axis)
    start = _rounded_point(start)
    end = _rounded_point(end)
    if edge.geom_type == GeomType.LINE:
        return OpenCircularSectionSegment("line", start, end)
    center = _rounded_point(_project(edge.arc_center, axis))
    try:
        return OpenCircularSectionSegment(
            "arc",
            start,
            end,
            center,
            round(float(edge.radius), _POINT_DIGITS),
            round(_arc_sweep(edge, start, end, axis), _ANGLE_DIGITS),
        )
    except ValueError:
        return None


def _orient_segments(
    segments: tuple[OpenCircularSectionSegment, ...],
) -> tuple[OpenCircularSectionSegment, ...] | None:
    remaining = list(segments)
    for first_index, first in enumerate(segments):
        for candidate_first in (first, first.reversed()):
            ordered = [candidate_first]
            unused = [item for index, item in enumerate(remaining) if index != first_index]
            while unused:
                matches = []
                for index, item in enumerate(unused):
                    for oriented in (item, item.reversed()):
                        if math.dist(ordered[-1].end, oriented.start) <= 2e-4:
                            matches.append((index, oriented))
                if len(matches) != 1:
                    break
                index, item = matches[0]
                ordered.append(item)
                unused.pop(index)
            if not unused:
                result = tuple(ordered)
                reverse = tuple(item.reversed() for item in reversed(result))
                return min(result, reverse)
    return None


def _material_fraction(part: Part, probe: Solid) -> float:
    result = part.intersect(probe)
    if result is None:
        return 0.0
    volume = intersection_volume(result)
    return volume / float(probe.volume)


def _floor_proof(
    part: Part, graph: FaceGraph, floor: FaceNode, axis: int, floor_at: float, mouth_at: float
) -> bool:
    distance = mouth_at - floor_at
    thickness = max(2e-5, abs(distance) * 1e-4)
    toward = [0.0] * 3
    behind = [0.0] * 3
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


def recognise_edge_open_circular_pockets(
    part: Part,
    *,
    face_edges: FaceEdges | None = None,
    ledger: ClaimLedger | EvidenceWriter | None = None,
) -> list[EdgeOpenCircularPocket]:
    """Recognise one-interrupted-end circular pockets from an exact open wall chain."""

    graph = FaceGraph(part, face_edges=face_edges) if ledger is None else ledger.graph
    found: list[tuple[EdgeOpenCircularPocket, tuple[FaceNode, ...], FaceNode]] = []
    for floor in graph.nodes:
        plane = _principal_plane(graph, floor) if graph.is_planar(floor) else None
        if plane is None:
            continue
        axis, floor_at = plane
        neighbours = tuple(graph.neighbours(floor))
        cylinders = tuple(
            node
            for node in neighbours
            if graph.face(node).geom_type == GeomType.CYLINDER
            and graph.arc(floor, node) == "concave"
        )
        walls = tuple(
            node
            for node in neighbours
            if graph.is_planar(node)
            and graph.arc(floor, node) == "concave"
            and (normal := graph.normal(node)) is not None
            and abs(normal[axis]) <= AXIS_ZERO_COS
        )
        boundary = (*cylinders, *walls)
        if (
            len(cylinders) != 2
            or len(walls) != 2
            or (ordered := _ordered_chain(graph, boundary)) is None
        ):
            continue
        if tuple(graph.face(node).geom_type for node in ordered) not in (
            (GeomType.CYLINDER, GeomType.PLANE, GeomType.CYLINDER, GeomType.PLANE),
            (GeomType.PLANE, GeomType.CYLINDER, GeomType.PLANE, GeomType.CYLINDER),
        ):
            continue
        spans = tuple(graph.bounds(node)[axis] for node in boundary)
        low = min(item[0] for item in spans)
        high = max(item[1] for item in spans)
        if high - low <= SPAN_EPS or any(
            abs(a - low) > SPAN_EPS or abs(b - high) > SPAN_EPS for a, b in spans
        ):
            continue
        if min(abs(floor_at - low), abs(floor_at - high)) > SPAN_EPS:
            continue
        mouth_at = high if abs(floor_at - low) <= SPAN_EPS else low
        mouth_context = set(graph.neighbours(boundary[0]))
        for node in boundary[1:]:
            mouth_context &= set(graph.neighbours(node))
        mouths = tuple(
            node
            for node in mouth_context
            if (mouth_plane := _principal_plane(graph, node)) is not None
            and mouth_plane[0] == axis
            and abs(mouth_plane[1] - mouth_at) <= SPAN_EPS
            and all(graph.arc(node, member) in ("convex", "smooth") for member in boundary)
        )
        owner = graph.common_valid_solid((*boundary, floor))
        if len(mouths) != 1 or owner is None:
            continue
        raw_segments = tuple(_segment(graph, floor, node, axis) for node in ordered)
        if any(item is None for item in raw_segments):
            continue
        segments = _orient_segments(tuple(item for item in raw_segments if item is not None))
        if segments is None:
            continue
        arcs = tuple(item for item in segments if item.kind == "arc")
        if (
            len(arcs) != 2
            or arcs[0].radius is None
            or arcs[1].radius is None
            or arcs[0].radius != arcs[1].radius
            or sum(abs(abs(item.sweep or 0.0) - math.pi) <= 1e-4 for item in arcs) != 1
            or not all(0 < abs(item.sweep or 0.0) <= math.pi + 1e-6 for item in arcs)
        ):
            continue
        if not _floor_proof(
            graph.solid_shape(owner),
            graph,
            floor,
            axis,
            floor_at,
            mouth_at,
        ):
            continue
        section = OpenCircularSection(segments, (segments[-1].end, segments[0].start))
        record = EdgeOpenCircularPocket(
            _AXES[axis],
            (round(min(floor_at, mouth_at), 3), round(max(floor_at, mouth_at), 3)),
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
                family=FamilyId.EDGE_OPEN_CIRCULAR_POCKETS,
                constituent=(*walls, floor),
            )
    return [record for record, _walls, _floor in found]
