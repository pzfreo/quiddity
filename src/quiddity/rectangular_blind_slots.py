# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Edge-open rectangular blind slots with one internal cap."""

from __future__ import annotations

from dataclasses import dataclass

from quiddity._adjacency import FaceGraph, FaceNode, axis_aligned_axis
from quiddity._candidates import EvidenceSink, FamilyId
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._record import Record
from quiddity._typing import Part
from quiddity.round_bottom_slots import (
    _common_convex_context,
    _coplanar_region,
    _empty_sweep,
    _length_tolerance,
    _principal_rectangle,
    _region_bounds,
    _region_face,
    _relation,
)

_AXES = "xyz"


@dataclass(frozen=True, order=True)
class RectangularBlindSlot(Record):
    """One capped, edge-open rectangular U-section slot."""

    axis: str
    open_sign: int
    length: float
    width_axis: str
    depth_axis: str
    depth_sign: int
    width: float
    depth: float
    at: tuple[float, float, float]


def _contains_span(
    actual: tuple[float, float], expected: tuple[float, float], nominal: float
) -> bool:
    tolerance = _length_tolerance(nominal)
    return actual[0] <= expected[0] + tolerance and actual[1] >= expected[1] - tolerance


def _has_unambiguous_slot_roles(length: float, width: float, depth: float) -> bool:
    """Require a run at least as wide as the section and distinctly longer than its depth."""

    tolerance = _length_tolerance(length, width, depth)
    return length + tolerance >= width and length > depth + tolerance


def _recognise_one(
    solid, graph: FaceGraph
) -> list[tuple[RectangularBlindSlot, frozenset[FaceNode]]]:
    solid_nodes = {graph.require_node(face) for face in solid.faces()}
    bounds = solid.bounding_box()
    envelope = (
        (bounds.min.X, bounds.max.X),
        (bounds.min.Y, bounds.max.Y),
        (bounds.min.Z, bounds.max.Z),
    )
    found: list[tuple[RectangularBlindSlot, frozenset[FaceNode]]] = []
    seen_caps: set[FaceNode] = set()
    for cap in sorted(solid_nodes, key=lambda node: node.index):
        if cap in seen_caps:
            continue
        cap_plane = axis_aligned_axis(graph.face(cap).wrapped)
        if cap_plane is None:
            continue
        concave_planar = sum(
            graph.is_planar(neighbour) and graph.arc(cap, neighbour) == "concave"
            for neighbour in graph.neighbours(cap)
        )
        if concave_planar < 2:
            continue
        cap_region = _coplanar_region(graph, cap) & solid_nodes
        seen_caps.update(cap_region)
        run, cap_station = cap_plane
        if any(
            axis_aligned_axis(graph.face(node).wrapped) != cap_plane for node in cap_region
        ) or not _principal_rectangle(graph, cap_region, run):
            continue
        neighbours = {
            node
            for source in cap_region
            for node in graph.neighbours(source)
            if node not in cap_region and graph.is_planar(node)
        }
        regions = {_coplanar_region(graph, node) & solid_nodes for node in neighbours}
        concave = tuple(
            region for region in regions if _relation(graph, cap_region, region) == "concave"
        )
        if len(concave) != 3:
            continue
        planes = []
        for region in concave:
            seed = min(region, key=lambda node: node.index)
            plane = axis_aligned_axis(graph.face(seed).wrapped)
            if (
                plane is None
                or plane[0] == run
                or any(  # pragma: no branch
                    axis_aligned_axis(graph.face(node).wrapped) != plane for node in region
                )
            ):
                break  # pragma: no cover - coplanar principal-plane regions establish one plane
            planes.append((region, plane))
        else:
            counts = {axis: sum(plane[0] == axis for _region, plane in planes) for axis in range(3)}
            width_axes = [axis for axis, count in counts.items() if count == 2]
            depth_axes = [axis for axis, count in counts.items() if count == 1]
            if len(width_axes) != 1 or len(depth_axes) != 1:
                continue
            width, depth = width_axes[0], depth_axes[0]
            if {run, width, depth} != {0, 1, 2}:  # pragma: no branch
                continue  # pragma: no cover - the three non-run plane counts establish all axes
            sides = tuple(region for region, plane in planes if plane[0] == width)
            (floor,) = tuple(region for region, plane in planes if plane[0] == depth)
            if any(not _principal_rectangle(graph, region, width) for region in sides) or not (
                _principal_rectangle(graph, floor, depth)
            ):
                continue
            side_planes = sorted(plane[1] for _region, plane in planes if plane[0] == width)
            floor_plane = next(plane for _region, plane in planes if plane[0] == depth)
            cap_bounds = _region_bounds(graph, cap_region)
            width_span = cap_bounds[width]
            depth_span = cap_bounds[depth]
            section_width = width_span[1] - width_span[0]
            section_depth = depth_span[1] - depth_span[0]
            section_tolerance = _length_tolerance(section_width, section_depth)
            if (  # pragma: no branch
                section_width <= 0
                or section_depth <= 0
                or any(
                    abs(actual - expected) > section_tolerance
                    for actual, expected in zip(side_planes, width_span, strict=True)
                )
                or min(abs(floor_plane[1] - end) for end in depth_span) > section_tolerance
                or _relation(graph, sides[0], floor) != "concave"
                or _relation(graph, sides[1], floor) != "concave"
            ):
                continue  # pragma: no cover - complete rectangular concave regions imply these
            depth_open = (
                depth_span[1]
                if abs(floor_plane[1] - depth_span[0]) <= section_tolerance
                else depth_span[0]
            )
            depth_sign = 1 if depth_open > floor_plane[1] else -1
            if not any(
                abs(depth_open - station) <= section_tolerance for station in envelope[depth]
            ):
                continue
            side_bounds = tuple(_region_bounds(graph, region) for region in sides)
            floor_bounds = _region_bounds(graph, floor)
            for open_sign, open_station in ((-1, envelope[run][0]), (1, envelope[run][1])):
                low, high = sorted((cap_station, open_station))
                length = high - low
                if length <= 0 or not _has_unambiguous_slot_roles(
                    length, section_width, section_depth
                ):
                    continue
                run_span = (low, high)
                if not all(
                    _contains_span(item[run], run_span, length)
                    for item in (*side_bounds, floor_bounds)
                ):
                    continue
                side_regions = (sides[0], floor, sides[1])
                if not _common_convex_context(graph, side_regions, run, open_station, length):
                    continue
                cap_face = _region_face(graph, cap_region)
                if cap_face is None:  # pragma: no cover - rectangle gate proved a sewable region
                    continue
                if not _empty_sweep(cap_face, solid, run, open_station - cap_station):
                    continue
                centre = [0.0, 0.0, 0.0]
                centre[run] = (low + high) / 2
                centre[width] = (width_span[0] + width_span[1]) / 2
                centre[depth] = (depth_span[0] + depth_span[1]) / 2
                record = RectangularBlindSlot(
                    axis=_AXES[run],
                    open_sign=open_sign,
                    length=round(length, 3),
                    width_axis=_AXES[width],
                    depth_axis=_AXES[depth],
                    depth_sign=depth_sign,
                    width=round(section_width, 3),
                    depth=round(section_depth, 3),
                    at=(
                        round(centre[0], 3),
                        round(centre[1], 3),
                        round(centre[2], 3),
                    ),
                )
                nodes = frozenset((*cap_region, *sides[0], *sides[1], *floor))
                found.append((record, nodes))
    # One original boundary cannot prove two different role assignments. Equal rediscovery is
    # harmless; competing records fail closed instead of choosing an axis or traversal order.
    occurrence = tuple[RectangularBlindSlot, frozenset[FaceNode]]
    by_nodes: dict[frozenset[FaceNode], dict[RectangularBlindSlot, occurrence]] = {}
    for record, nodes in found:
        by_nodes.setdefault(nodes, {})[record] = (record, nodes)
    return [next(iter(records.values())) for records in by_nodes.values() if len(records) == 1]


def recognise_rectangular_blind_slots(
    part: Part, *, ledger: ClaimLedger | EvidenceWriter | None = None
) -> list[RectangularBlindSlot]:
    """Recognise the principal-axis, one-cap rectangular U-section subset."""

    graph = ledger.graph if ledger is not None else FaceGraph(part)
    sink: EvidenceSink | None = None if ledger is None else ledger.sink
    found: list[tuple[RectangularBlindSlot, frozenset[FaceNode]]] = []
    for solid in part.solids():
        if solid.is_valid:
            found.extend(_recognise_one(solid, graph))
    found.sort(key=lambda item: item[0])
    if sink is not None:
        found = [item for item in found if graph.common_valid_solid(item[1]) is not None]
        for record, nodes in found:
            sink.propose(FamilyId.RECTANGULAR_BLIND_SLOTS, record, defining=nodes)
    return [record for record, _nodes in found]
