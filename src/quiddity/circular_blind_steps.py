# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Recognition of bounded quarter-cylindrical blind corner steps.

The supported feature is an inward quarter-cylinder cut from a stock corner, open at one
principal-axis envelope end and closed by one perpendicular interior sector terminal. Two convex
principal side joins prove the corner opening; an exact sweep of the terminal sector proves the
removed volume is empty. Full blind bores, through grooves, external rounds, other angular spans,
enclosed pockets and obstructed cuts fail closed without a size threshold.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Any

from build123d import Vector, extrude

from quiddity._adjacency import FaceGraph, FaceNode, axis_aligned_axis
from quiddity._candidates import EvidenceSink, FamilyId
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._cylinder_substrate import analyse_cylinders
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact,
    EffectiveFaceSurfaceQuery,
    SurfaceKind,
    SurfaceUse,
    SurfaceUseRefusal,
    cylinder_surface_dependency,
    effective_faces_for_graph,
)
from quiddity._geometry import COORD_FLOOR, SMOOTH_ARC_GAP, quantise
from quiddity._record import Record
from quiddity._typing import (
    CylinderEvidence,
    CylinderInventory,
    FrozenCylinderInventory,
    Part,
)
from quiddity._volume_probe import intersection_volume

_AXES = "xyz"
#: Radians of OCCT cylindrical parameter noise admitted around an exact quarter turn. This is a
#: parameter-space read tolerance, not a feature-size gate. At unit radius its maximum boundary
#: displacement is below 0.1 micrometre. Tests pin both sides of the boundary.
QUARTER_TURN_RAD_TOL = 1e-7
_PRINCIPAL_AXIS_COS = 1.0 - SMOOTH_ARC_GAP

Point3 = tuple[float, float, float]
Point2 = tuple[float, float]


@dataclass(frozen=True, order=True)
class CircularBlindStep(Record):
    """One quarter-cylindrical corner cut with a blind interior terminal.

    ``centreline`` is ordered from the blind terminal to the open envelope, so it preserves run
    direction as well as location. ``section`` is the canonical transverse arc endpoint, cylinder
    centre and other arc endpoint in ascending global-axis coordinate order (``yz`` for x, ``xz``
    for y, ``xy`` for z). Together they locate the occupied quadrant without topology access.
    """

    axis: str
    radius: float
    length: float
    centreline: tuple[Point3, Point3]
    section: tuple[Point2, Point2, Point2]


def _is_concave(graph: FaceGraph, left: FaceNode, right: FaceNode) -> bool:
    return graph.arc(left, right) == "concave"


def _is_convex(graph: FaceGraph, left: FaceNode, right: FaceNode) -> bool:
    return graph.arc(left, right) == "convex"


def _axis_bounds(shape: Any, axis: int) -> tuple[float, float]:
    bounds = shape.bounding_box()
    return (
        (bounds.min.X, bounds.max.X),
        (bounds.min.Y, bounds.max.Y),
        (bounds.min.Z, bounds.max.Z),
    )[axis]


def _principal_axis(evidence: CylinderEvidence) -> int | None:
    axis = _AXES.index(evidence["axis"])
    direction = evidence["dir_xyz"]
    return axis if abs(direction[axis]) >= _PRINCIPAL_AXIS_COS else None


def _empty_terminal_sweep(
    graph: FaceGraph,
    cylinder: FaceNode,
    terminal: FaceNode,
    *,
    axis: int,
    direction: int,
    length: float,
) -> bool:
    solid_ref = graph.common_valid_solid((cylinder, terminal))
    if solid_ref is None:
        return False
    vector = [0.0, 0.0, 0.0]
    vector[axis] = float(direction)
    swept = extrude(graph.face(terminal), amount=length, dir=Vector(*vector))
    intersection: Any = swept.intersect(graph.solid_shape(solid_ref))
    occupied = intersection_volume(intersection)
    return bool(occupied == 0.0)


def _shared_arc_endpoints(
    graph: FaceGraph, cylinder: FaceNode, terminal: FaceNode, axis: int
) -> tuple[Point2, Point2] | None:
    transverse = [index for index in range(3) if index != axis]
    points: list[Point2] = []
    for edge in graph.shared_edges(cylinder, terminal):
        for vertex in edge.vertices():
            point = tuple(vertex)
            projected = (float(point[transverse[0]]), float(point[transverse[1]]))
            if not any(math.dist(projected, existing) <= COORD_FLOOR for existing in points):
                points.append(projected)
    if len(points) != 2:
        return None
    ordered = sorted(points)
    return ordered[0], ordered[1]


def _candidate(
    graph: FaceGraph,
    cylinder: FaceNode,
    terminal: FaceNode,
    evidence: CylinderEvidence,
) -> CircularBlindStep | None:
    axis = _principal_axis(evidence)
    plane = axis_aligned_axis(graph.face(terminal).wrapped)
    if (
        axis is None
        or plane is None
        or plane[0] != axis
        or not _is_concave(graph, cylinder, terminal)
    ):
        return None
    if (
        not math.isclose(
            evidence["u_extent"],
            math.pi / 2,
            rel_tol=0.0,
            abs_tol=QUARTER_TURN_RAD_TOL,
        )
        or evidence["external"]
    ):
        return None

    solid_ref = graph.common_valid_solid((cylinder, terminal))
    if solid_ref is None:
        return None
    low, high = graph.bounds(cylinder)[axis]
    solid_low, solid_high = _axis_bounds(graph.solid_shape(solid_ref), axis)
    terminal_at = plane[1]
    if math.isclose(terminal_at, low, abs_tol=COORD_FLOOR) and math.isclose(
        high, solid_high, abs_tol=COORD_FLOOR
    ):
        direction = 1
        opening_at = high
    elif math.isclose(terminal_at, high, abs_tol=COORD_FLOOR) and math.isclose(
        low, solid_low, abs_tol=COORD_FLOOR
    ):
        direction = -1
        opening_at = low
    else:
        return None
    length = high - low

    axial: list[FaceNode] = []
    sides: list[FaceNode] = []
    for neighbour in graph.neighbours(cylinder):
        if neighbour == terminal:
            continue
        neighbour_plane = axis_aligned_axis(graph.face(neighbour).wrapped)
        if neighbour_plane is None or not _is_convex(graph, cylinder, neighbour):
            return None
        (axial if neighbour_plane[0] == axis else sides).append(neighbour)
    transverse_axes = {index for index in range(3) if index != axis}
    side_axes = {
        plane[0]
        for node in sides
        if (plane := axis_aligned_axis(graph.face(node).wrapped)) is not None
    }
    if len(axial) != 1 or len(sides) != 2 or side_axes != transverse_axes:
        return None
    axial_plane = axis_aligned_axis(graph.face(axial[0]).wrapped)
    if axial_plane is None or not math.isclose(axial_plane[1], opening_at, abs_tol=COORD_FLOOR):
        return None
    if not _empty_terminal_sweep(
        graph,
        cylinder,
        terminal,
        axis=axis,
        direction=direction,
        length=length,
    ):
        return None

    endpoints = _shared_arc_endpoints(graph, cylinder, terminal, axis)
    if endpoints is None:
        return None
    transverse = [index for index in range(3) if index != axis]
    anchor = evidence["axis_xyz"]
    centre = (anchor[transverse[0]], anchor[transverse[1]])
    terminal_point = list(anchor)
    opening_point = list(anchor)
    terminal_point[axis] = terminal_at
    opening_point[axis] = opening_at

    def point2(point: Point2) -> Point2:
        return (quantise(point[0]), quantise(point[1]))

    def point3(point: list[float]) -> Point3:
        return (quantise(point[0]), quantise(point[1]), quantise(point[2]))

    return CircularBlindStep(
        axis=_AXES[axis],
        radius=quantise(evidence["diameter"] / 2),
        length=quantise(length),
        centreline=(point3(terminal_point), point3(opening_point)),
        section=(point2(endpoints[0]), point2(centre), point2(endpoints[1])),
    )


def _discover_circular_blind_steps(
    part: Part,
    *,
    graph: FaceGraph,
    cylinders: CylinderInventory | FrozenCylinderInventory,
    effective: EffectiveFaceSurfaceQuery,
    sink: EvidenceSink | None,
) -> list[CircularBlindStep]:
    evidence_by_node = {
        graph.require_node(item["face"]): item for item in (*cylinders[0], *cylinders[1])
    }
    proposals: list[
        tuple[CircularBlindStep, tuple[FaceNode, FaceNode], tuple[SurfaceUse, SurfaceUse]]
    ] = []
    for cylinder, evidence in sorted(evidence_by_node.items(), key=lambda item: item[0].index):
        for terminal in graph.neighbours(cylinder):
            terminal_fact = effective.fact(graph.face(terminal))
            if not isinstance(terminal_fact, AnalyticSurfaceFact) or (
                terminal_fact.kind is not SurfaceKind.PLANE
            ):
                continue
            record = _candidate(graph, cylinder, terminal, evidence)
            if record is None:
                continue
            cylinder_use = cylinder_surface_dependency(effective, graph.face(cylinder))
            terminal_use = effective.use(graph.face(terminal))
            if isinstance(cylinder_use, SurfaceUseRefusal) or isinstance(
                terminal_use, SurfaceUseRefusal
            ):
                raise ValueError("CircularBlindStep surface provenance is unavailable")
            proposals.append((record, (cylinder, terminal), (cylinder_use, terminal_use)))
    proposals.sort(key=lambda proposal: proposal[0])
    if sink is not None:
        for _record, nodes, _uses in proposals:
            if graph.common_valid_solid(nodes) is None:
                raise ValueError(
                    "CircularBlindStep defining faces do not belong to one valid solid"
                )
        for record, nodes, uses in proposals:
            sink.propose(
                FamilyId.CIRCULAR_BLIND_STEPS,
                record,
                defining=nodes,
                surfaces=uses,
            )
    return [record for record, _nodes, _uses in proposals]


def recognise_circular_blind_steps(
    part: Part,
    *,
    cyls: CylinderInventory | FrozenCylinderInventory | None = None,
    ledger: ClaimLedger | EvidenceWriter | None = None,
) -> list[CircularBlindStep]:
    """Recognise bounded quarter-cylindrical blind corner steps."""

    graph = FaceGraph(part) if ledger is None else ledger.graph
    effective = effective_faces_for_graph(graph)
    cylinders = analyse_cylinders(part, face_surfaces=effective) if cyls is None else cyls
    sink = None if ledger is None else ledger.sink
    return _discover_circular_blind_steps(
        part,
        graph=graph,
        cylinders=cylinders,
        effective=effective,
        sink=sink,
    )
