# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Conservative rectangular through-step recognition.

The supported occurrence is exactly two principal-plane regions joined by one concave seam, open
across the complete run of one valid source solid.  The open section records the removed quadrant
explicitly. Boundary interruptions from independent geometry are permitted only when the complete
seam, envelope, terminals and empty removed prism remain proved. Channels, pockets, capped cuts,
tapered or curved walls, seam interruptions and partial-run steps remain outside this family.
"""

from __future__ import annotations

from dataclasses import dataclass
from functools import total_ordering
from typing import Any

from build123d import GeomType, Wire

from quiddity._adjacency import FaceGraph, FaceNode, axis_aligned_axis
from quiddity._body_identity import BodyKey, unambiguous_body_keys
from quiddity._candidates import EvidenceSink, FamilyId
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._geometry import COORD_FLOOR, SMOOTH_ARC_GAP
from quiddity._record import Record
from quiddity._typing import Part
from quiddity._volume_probe import prism_is_empty

_AXES = "xyz"
SPAN_EPS = COORD_FLOOR


@total_ordering
@dataclass(frozen=True)
class ThroughStep(Record):
    """One principal-axis rectangular open-profile step spanning a source solid.

    ``section`` is the canonical three-point open polyline perpendicular to ``axis``: envelope
    endpoint, concave corner, envelope endpoint. Each pair uses the two non-run coordinates in
    ascending ``x``, ``y``, ``z`` order (``yz`` for an x run, ``xz`` for y, ``xy`` for z).
    ``at`` is the caller-coordinate midpoint of the proved empty removed prism. Its orientation,
    anchor and both leg dimensions are therefore explicit rather than inferred from unsigned
    widths.
    """

    axis: str
    length: float
    at: tuple[float, float, float]
    section: tuple[
        tuple[float, float],
        tuple[float, float],
        tuple[float, float],
    ]
    body_key: BodyKey | None = ()

    def __lt__(self, other: object) -> bool:
        if not isinstance(other, ThroughStep):
            return NotImplemented
        return self._order_key() < other._order_key()

    def _order_key(self) -> tuple[object, ...]:
        return (
            self.axis,
            self.length,
            self.at,
            self.section,
            self.body_key is not None,
            self.body_key or (),
        )


@dataclass(frozen=True)
class _Region:
    nodes: tuple[FaceNode, ...]
    normal_axis: int
    coordinate: float
    bounds: tuple[tuple[float, float], tuple[float, float], tuple[float, float]]


def _four_principal_runs(wire: Wire, normal_axis: int) -> bool:
    edges = wire.edges()
    if any(edge.geom_type != GeomType.LINE for edge in edges):
        return False
    directions = [edge.tangent_at() for edge in edges]
    if not directions:
        return False
    runs = [directions[0]]
    for direction in directions[1:]:
        if 1.0 - runs[-1].dot(direction) > SMOOTH_ARC_GAP:
            runs.append(direction)
    if len(runs) > 1 and 1.0 - runs[-1].dot(runs[0]) <= SMOOTH_ARC_GAP:
        runs.pop()
    in_plane = [axis for axis in range(3) if axis != normal_axis]
    run_axes: list[int] = []
    for direction in runs:
        aligned = [
            axis
            for axis in in_plane
            if 1.0 - abs(getattr(direction, _AXES[axis].upper())) <= SMOOTH_ARC_GAP
        ]
        if len(aligned) != 1:
            return False
        run_axes.append(aligned[0])
    return len(run_axes) == 4 and all(run_axes.count(axis) == 2 for axis in in_plane)


def _coplanar_region(
    graph: FaceGraph,
    seed: FaceNode,
    planes: dict[FaceNode, tuple[int, float] | None],
) -> frozenset[FaceNode]:
    """Return the connected same-principal-plane subset containing ``seed``."""

    plane = planes[seed]
    if plane is None:
        return frozenset()
    found = {seed}
    pending = [seed]
    while pending:
        current = pending.pop()
        for neighbour in graph.neighbours(current):
            if neighbour in found:
                continue
            other = planes[neighbour]
            if (
                graph.arc(current, neighbour) == "smooth"
                and other is not None
                and other[0] == plane[0]
                and abs(other[1] - plane[1]) <= COORD_FLOOR
            ):
                found.add(neighbour)
                pending.append(neighbour)
    return frozenset(found)


def _regions(
    graph: FaceGraph,
    solid_nodes: set[FaceNode],
    planes: dict[FaceNode, tuple[int, float] | None],
) -> list[_Region]:
    out: list[_Region] = []
    seen: set[FaceNode] = set()
    for seed in sorted(solid_nodes, key=lambda node: node.index):
        if seed in seen:
            continue
        plane = planes[seed]
        if plane is None:
            continue
        region = _coplanar_region(graph, seed, planes) & solid_nodes
        seen.update(region)
        axis, coordinate = plane
        measured = tuple(
            (
                min(graph.bounds(node)[index][0] for node in region),
                max(graph.bounds(node)[index][1] for node in region),
            )
            for index in range(3)
        )
        bounds = (measured[0], measured[1], measured[2])
        out.append(
            _Region(tuple(sorted(region, key=lambda node: node.index)), axis, coordinate, bounds)
        )
    return out


def _relation(graph: FaceGraph, left: _Region, right: _Region) -> str | None:
    kinds: set[str] = set()
    for a in left.nodes:
        for b in right.nodes:
            kind = graph.arc(a, b)
            if kind is not None:
                kinds.add(kind)
    return kinds.pop() if len(kinds) == 1 else None


def _shared_run_is_complete(
    graph: FaceGraph, left: _Region, right: _Region, run: int, low: float, high: float
) -> bool:
    shared = [edge for a in left.nodes for b in right.nodes for edge in graph.shared_edges(a, b)]
    if not shared or any(edge.geom_type != GeomType.LINE for edge in shared):
        return False
    intervals: list[tuple[float, float]] = []
    for edge in shared:
        bounds = edge.bounding_box()
        by_axis = (
            (bounds.min.X, bounds.max.X),
            (bounds.min.Y, bounds.max.Y),
            (bounds.min.Z, bounds.max.Z),
        )
        if any(
            abs(getattr(edge.tangent_at(), _AXES[axis].upper())) > SMOOTH_ARC_GAP
            for axis in range(3)
            if axis != run
        ):
            return False
        intervals.append(by_axis[run])
    merged: list[list[float]] = []
    for start, end in sorted(intervals):
        if not merged or start > merged[-1][1] + SPAN_EPS:
            merged.append([start, end])
        else:
            merged[-1][1] = max(merged[-1][1], end)
    return (
        len(merged) == 1
        and abs(merged[0][0] - low) <= SPAN_EPS
        and abs(merged[0][1] - high) <= SPAN_EPS
    )


def _common_terminal(
    graph: FaceGraph,
    left: _Region,
    right: _Region,
    run: int,
    station: float,
    spans: dict[str, tuple[float, float]],
    planes: dict[FaceNode, tuple[int, float] | None],
) -> bool:
    left_neighbours = {node for source in left.nodes for node in graph.neighbours(source)}
    right_neighbours = {node for source in right.nodes for node in graph.neighbours(source)}
    seen: set[FaceNode] = set()
    for seed in sorted(left_neighbours | right_neighbours, key=lambda node: node.index):
        if seed in seen:
            continue
        region = _coplanar_region(graph, seed, planes)
        seen.update(region)
        plane = planes[seed]
        if plane is None or plane[0] != run or abs(plane[1] - station) > SPAN_EPS:
            continue
        if not (region & left_neighbours and region & right_neighbours):
            continue
        measured = tuple(
            (
                min(graph.bounds(node)[axis][0] for node in region),
                max(graph.bounds(node)[axis][1] for node in region),
            )
            for axis in range(3)
        )
        if any(
            measured[axis][0] > spans[_AXES[axis]][0] + SPAN_EPS
            or measured[axis][1] < spans[_AXES[axis]][1] - SPAN_EPS
            for axis in range(3)
            if axis != run
        ):
            continue
        left_arcs = []
        for source in left.nodes:
            for node in region & set(graph.neighbours(source)):
                kind = graph.arc(source, node)
                left_arcs.append(kind)
        right_arcs = []
        for source in right.nodes:
            for node in region & set(graph.neighbours(source)):
                kind = graph.arc(source, node)
                right_arcs.append(kind)
        if left_arcs and right_arcs and all(kind == "convex" for kind in (*left_arcs, *right_arcs)):
            return True
    return False


def _section_and_spans(
    left: _Region, right: _Region, run: int, solid_bounds: tuple[tuple[float, float], ...]
) -> tuple[tuple[tuple[float, float], ...], dict[str, tuple[float, float]]] | None:
    section_axes = [axis for axis in range(3) if axis != run]
    if {left.normal_axis, right.normal_axis} != set(section_axes):
        return None
    corner = {left.normal_axis: left.coordinate, right.normal_axis: right.coordinate}
    endpoint: dict[int, float] = {}
    for region, varying in ((left, right.normal_axis), (right, left.normal_axis)):
        low, high = region.bounds[varying]
        at = corner[varying]
        candidates = [
            value
            for value, envelope in (
                (low, solid_bounds[varying][0]),
                (high, solid_bounds[varying][1]),
            )
            if abs(value - envelope) <= SPAN_EPS and abs(value - at) > SPAN_EPS
        ]
        if len(candidates) != 1:
            return None
        endpoint[varying] = candidates[0]
    a, b = section_axes
    points = (
        (corner[a], endpoint[b]) if left.normal_axis == a else (endpoint[a], corner[b]),
        (corner[a], corner[b]),
        (endpoint[a], corner[b]) if right.normal_axis == b else (corner[a], endpoint[b]),
    )
    canonical = min(points, tuple(reversed(points)))
    spans: dict[str, tuple[float, float]] = {
        _AXES[run]: (left.bounds[run][0], left.bounds[run][1]),
        _AXES[a]: (min(corner[a], endpoint[a]), max(corner[a], endpoint[a])),
        _AXES[b]: (min(corner[b], endpoint[b]), max(corner[b], endpoint[b])),
    }
    return canonical, spans


def _recognise_one(
    solid: Any,
    graph: FaceGraph,
    planes: dict[FaceNode, tuple[int, float] | None],
    body_key: BodyKey | None,
) -> list[tuple[ThroughStep, tuple[FaceNode, ...]]]:
    solid_nodes = {graph.require_node(face) for face in solid.faces()}
    regions = _regions(graph, solid_nodes, planes)
    bounds = graph.solid_properties.bounding_box(solid)
    solid_bounds = (
        (bounds.min.X, bounds.max.X),
        (bounds.min.Y, bounds.max.Y),
        (bounds.min.Z, bounds.max.Z),
    )
    out: list[tuple[ThroughStep, tuple[FaceNode, ...]]] = []
    claimed: set[frozenset[FaceNode]] = set()
    for index, left in enumerate(regions):
        for right in regions[index + 1 :]:
            if left.normal_axis == right.normal_axis:
                continue
            run = 3 - left.normal_axis - right.normal_axis
            low, high = left.bounds[run]
            # Reject non-spanning pairs on cached bounds before asking the graph for arc geometry.
            if (
                abs(low - right.bounds[run][0]) > SPAN_EPS
                or abs(high - right.bounds[run][1]) > SPAN_EPS
                or abs(low - solid_bounds[run][0]) > SPAN_EPS
                or abs(high - solid_bounds[run][1]) > SPAN_EPS
                or _relation(graph, left, right) != "concave"
            ):
                continue
            measured = _section_and_spans(left, right, run, solid_bounds)
            if measured is None:
                continue
            section, spans = measured
            if (
                not _shared_run_is_complete(graph, left, right, run, low, high)
                or not _common_terminal(graph, left, right, run, low, spans, planes)
                or not _common_terminal(graph, left, right, run, high, spans, planes)
            ):
                continue
            if any(
                candidate not in (left, right)
                and abs(candidate.bounds[run][0] - low) <= SPAN_EPS
                and abs(candidate.bounds[run][1] - high) <= SPAN_EPS
                and (
                    _relation(graph, left, candidate) == "concave"
                    or _relation(graph, right, candidate) == "concave"
                )
                for candidate in regions
            ):
                continue
            if not prism_is_empty(spans, solid, inset=COORD_FLOOR):
                continue
            nodes = frozenset((*left.nodes, *right.nodes))
            if nodes in claimed:
                continue
            claimed.add(nodes)
            ordered = tuple(node for node in graph.nodes if node in nodes)
            if graph.common_valid_solid(ordered) is None:
                raise ValueError("ThroughStep defining faces do not belong to one valid solid")
            at = tuple(sum(spans[_AXES[axis]]) / 2 for axis in range(3))
            rounded = tuple((round(point[0], 3), round(point[1], 3)) for point in section)
            record = ThroughStep(
                _AXES[run],
                round(high - low, 3),
                (round(at[0], 3), round(at[1], 3), round(at[2], 3)),
                (rounded[0], rounded[1], rounded[2]),
                body_key,
            )
            out.append((record, ordered))
    return out


def recognise_through_steps(
    part: Part, *, ledger: ClaimLedger | EvidenceWriter | None = None
) -> list[ThroughStep]:
    """Recognise the proven rectangular subset of open-profile through steps."""

    graph = FaceGraph(part) if ledger is None else ledger.graph
    sink: EvidenceSink | None = None if ledger is None else ledger.sink
    planes = {node: axis_aligned_axis(graph.face(node).wrapped) for node in graph.nodes}
    solids = list(part.solids())
    body_keys = unambiguous_body_keys(
        solids, require_valid_solid=True, properties=graph.solid_properties
    )
    proposals = [
        proposal
        for solid, body_key in zip(solids, body_keys, strict=True)
        for proposal in _recognise_one(solid, graph, planes, body_key)
    ]
    proposals.sort(key=lambda proposal: proposal[0])
    if sink is not None:
        # Validate the complete batch before the first append-only issuance. A stale or foreign
        # node in a later occurrence must not leave an earlier partial family prefix.
        for _record, nodes in proposals:
            if graph.common_valid_solid(nodes) is None:
                raise ValueError("ThroughStep defining faces do not belong to one valid solid")
        for record, nodes in proposals:
            sink.propose(FamilyId.THROUGH_STEPS, record, defining=nodes)
    return [record for record, _nodes in proposals]
