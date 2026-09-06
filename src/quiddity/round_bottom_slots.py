# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Edge-open, round-bottom blind slots with one internal cap.

This is deliberately not the rectangular :class:`Pocket` family.  Its constant section is an
open U: a flat floor joined tangentially to two equal quarter-cylinders, open at a source-solid
depth envelope and swept from a run-envelope mouth to one planar blind cap.
"""

from __future__ import annotations

from dataclasses import dataclass, replace
from math import pi

from build123d import Face, GeomType, Solid, Vector, Wire
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Cylinder
from OCP.Standard import Standard_Failure

from quiddity._adjacency import FaceGraph, FaceNode, axis_aligned_axis, is_any_smooth
from quiddity._candidates import EvidenceSink, FamilyId
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._geometry import (
    AXIS_ALIGNED_COS,
    AXIS_ZERO_COS,
    COORD_FLOOR,
    SMOOTH_ARC_GAP,
    length_tol,
)
from quiddity._record import Record
from quiddity._typing import EdgeLike, Part
from quiddity._volume_probe import intersection_volume

_AXES = "xyz"
_LENGTH_REL = 1e-7


def _length_tolerance(*values: float) -> float:
    """Kernel-coordinate equality scaled to the smallest relevant local geometry."""

    return length_tol(max((abs(value) for value in values), default=0.0), rel=_LENGTH_REL)


@dataclass(frozen=True, order=True)
class RoundBottomBlindSlot(Record):
    """One capped, edge-open U-section slot.

    ``axis`` is the penetration/run direction and ``open_sign`` identifies its source-envelope
    mouth. ``depth_sign`` identifies the material-outward opening of the U section along
    ``depth_axis``. ``flat_width`` and ``radius`` define the section: total opening width is
    ``flat_width + 2 * radius`` and profile depth is ``radius``.
    """

    axis: str
    open_sign: int
    length: float
    width_axis: str
    depth_axis: str
    depth_sign: int
    radius: float
    flat_width: float
    at: tuple[float, float, float]

    @property
    def width(self) -> float:
        return self.flat_width + 2 * self.radius

    @property
    def depth(self) -> float:
        return self.radius


@dataclass(frozen=True)
class _Cylinder:
    nodes: frozenset[FaceNode]
    radius: float
    axis: int
    centre: tuple[float, float, float]


def _cylinder_surface(
    graph: FaceGraph, node: FaceNode
) -> tuple[float, int, tuple[float, float, float]] | None:
    surface = BRepAdaptor_Surface(graph.face(node).wrapped)
    if surface.GetType() != GeomAbs_Cylinder:
        return None
    cylinder = surface.Cylinder()
    direction = cylinder.Axis().Direction()
    components = (direction.X(), direction.Y(), direction.Z())
    aligned = [
        axis
        for axis, value in enumerate(components)
        if abs(value) >= AXIS_ALIGNED_COS
        and all(abs(other) <= AXIS_ZERO_COS for i, other in enumerate(components) if i != axis)
    ]
    if len(aligned) != 1:
        return None
    location = cylinder.Axis().Location()
    return (
        float(cylinder.Radius()),
        aligned[0],
        (float(location.X()), float(location.Y()), float(location.Z())),
    )


def _same_cylinder(
    left: tuple[float, int, tuple[float, float, float]],
    right: tuple[float, int, tuple[float, float, float]],
) -> bool:
    radius, axis, centre = left
    other_radius, other_axis, other_centre = right
    return (
        axis == other_axis
        and abs(radius - other_radius) <= _length_tolerance(radius, other_radius)
        and all(
            abs(centre[i] - other_centre[i]) <= _length_tolerance(radius, other_radius)
            for i in range(3)
            if i != axis
        )
    )


def _cylinder_region(graph: FaceGraph, seed: FaceNode) -> _Cylinder | None:
    surface = _cylinder_surface(graph, seed)
    if surface is None:
        return None
    found = {seed}
    pending = [seed]
    while pending:
        current = pending.pop()
        for neighbour in graph.neighbours(current):
            candidate = _cylinder_surface(graph, neighbour)
            if (
                neighbour in found
                or candidate is None
                or not _same_cylinder(surface, candidate)
                or not is_any_smooth(graph.arc(current, neighbour))
            ):
                continue
            found.add(neighbour)
            pending.append(neighbour)
    radius, axis, centre = surface
    return _Cylinder(frozenset(found), radius, axis, centre)


def _region_boundary_wire(
    graph: FaceGraph, nodes: frozenset[FaceNode], *, planar: bool = True
) -> Wire | None:
    if not nodes or any(not graph.face(node).is_valid for node in nodes):
        return None
    uses: dict[EdgeLike, int] = {}
    for node in nodes:
        for edge in graph.edges(node):
            uses[edge] = uses.get(edge, 0) + 1
    if any(count > 2 for count in uses.values()):
        return None
    boundary = [edge for edge, count in uses.items() if count == 1]
    try:
        wires = list(Wire.combine(boundary, tol=COORD_FLOOR))
    except (Standard_Failure, RuntimeError, ValueError):
        return None
    if len(wires) != 1 or not wires[0].is_closed:
        return None
    if not planar:
        return wires[0]
    return wires[0] if _validated_planar_face(wires[0]) is not None else None


def _region_face(graph: FaceGraph, nodes: frozenset[FaceNode]) -> Face | None:
    wire = _region_boundary_wire(graph, nodes, planar=False)
    if wire is None:
        return None
    return _validated_planar_face(wire)


def _validated_planar_face(wire: Wire) -> Face | None:
    """Refuse kernel construction failure, but do not hide programming/invariant errors."""
    try:
        face = Face(wire)
        return face if face.is_valid else None
    except (Standard_Failure, RuntimeError, ValueError):
        return None


def _coplanar_region(graph: FaceGraph, seed: FaceNode) -> frozenset[FaceNode]:
    """Connected same-principal-plane patches, without crossing a tangent blend."""

    plane = axis_aligned_axis(graph.face(seed).wrapped)
    if plane is None:
        return frozenset()
    found = {seed}
    pending = [seed]
    seed_scale = max(
        high - low for axis, (low, high) in enumerate(graph.bounds(seed)) if axis != plane[0]
    )
    while pending:
        current = pending.pop()
        for neighbour in graph.neighbours(current):
            if neighbour in found or not is_any_smooth(graph.arc(current, neighbour)):
                continue
            other = axis_aligned_axis(graph.face(neighbour).wrapped)
            if (
                other is not None
                and other[0] == plane[0]
                and abs(other[1] - plane[1])
                <= _length_tolerance(
                    seed_scale,
                    *(
                        high - low
                        for axis, (low, high) in enumerate(graph.bounds(neighbour))
                        if axis != plane[0]
                    ),
                )
            ):
                found.add(neighbour)
                pending.append(neighbour)
    return frozenset(found)


def _relation(
    graph: FaceGraph, left: frozenset[FaceNode], right: frozenset[FaceNode]
) -> str | None:
    kinds = set()
    for a in left:
        for b in right:
            kind = graph.arc(a, b)
            if kind is not None:
                kinds.add(kind)
    return kinds.pop() if len(kinds) == 1 else None


def _region_bounds(
    graph: FaceGraph, nodes: frozenset[FaceNode]
) -> tuple[tuple[float, float], tuple[float, float], tuple[float, float]]:
    values = tuple(
        (
            min(graph.bounds(node)[axis][0] for node in nodes),
            max(graph.bounds(node)[axis][1] for node in nodes),
        )
        for axis in range(3)
    )
    return values  # type: ignore[return-value]


def _principal_rectangle(graph: FaceGraph, nodes: frozenset[FaceNode], normal_axis: int) -> bool:
    """Whether a logical planar region is one valid hole-free principal rectangle."""

    wire = _region_boundary_wire(graph, nodes)
    if wire is None:
        return False
    edges = wire.edges()
    if not edges or any(edge.geom_type != GeomType.LINE for edge in edges):
        return False
    directions = [edge.tangent_at() for edge in edges]
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


def _boundary_runs(wire: Wire) -> list[tuple[GeomType, list[EdgeLike]]] | None:
    """Co-directed straight or same-radius circular runs around one boundary."""
    edges = wire.edges()
    if not edges or any(edge.geom_type not in (GeomType.LINE, GeomType.CIRCLE) for edge in edges):
        return None
    groups: list[tuple[GeomType, list[EdgeLike]]] = []
    for edge in edges:
        if groups and groups[-1][0] == edge.geom_type:
            kind, members = groups[-1]
            if (
                (
                    kind == GeomType.LINE
                    and 1.0 - members[-1].tangent_at().dot(edge.tangent_at()) > SMOOTH_ARC_GAP
                )
                or kind == GeomType.CIRCLE
                and abs(members[-1].radius - edge.radius)
                > _length_tolerance(members[-1].radius, edge.radius)
            ):
                groups.append((edge.geom_type, [edge]))
            else:
                members.append(edge)
        else:
            groups.append((edge.geom_type, [edge]))
    if len(groups) > 1 and groups[0][0] == groups[-1][0]:
        kind, members = groups[0]
        tail = groups[-1][1]
        compatible = (
            kind == GeomType.LINE
            and 1.0 - tail[-1].tangent_at().dot(members[0].tangent_at()) <= SMOOTH_ARC_GAP
        ) or (
            kind == GeomType.CIRCLE
            and abs(tail[-1].radius - members[0].radius)
            <= _length_tolerance(tail[-1].radius, members[0].radius)
        )
        if compatible:
            groups[0] = kind, [*groups.pop()[1], *members]
    return groups


def _alternating_profile_runs(
    wire: Wire,
) -> tuple[list[list[EdgeLike]], list[list[EdgeLike]]] | None:
    """The two co-directed straight and two same-circle runs of one U boundary."""

    groups = _boundary_runs(wire)
    if groups is None:
        return None
    if len(groups) != 4 or any(
        groups[index][0] == groups[(index + 1) % 4][0] for index in range(4)
    ):
        return None
    return (
        [members for kind, members in groups if kind == GeomType.LINE],
        [members for kind, members in groups if kind == GeomType.CIRCLE],
    )


def _same_span(
    graph: FaceGraph, regions: tuple[frozenset[FaceNode], ...], axis: int
) -> tuple[float, float] | None:
    spans = [_region_bounds(graph, region)[axis] for region in regions]
    low, high = spans[0]
    nominal = high - low
    tolerance = _length_tolerance(nominal)
    if all(abs(a - low) <= tolerance and abs(b - high) <= tolerance for a, b in spans[1:]):
        return low, high
    return None


def _quarter_cylinder(graph: FaceGraph, cylinder: _Cylinder, run_span: tuple[float, float]) -> bool:
    wire = _region_boundary_wire(graph, cylinder.nodes, planar=False)
    runs = _alternating_profile_runs(wire) if wire is not None else None
    if runs is None:
        return False
    lines, arcs = runs
    length = run_span[1] - run_span[0]
    return (
        len(lines) == 2
        and len(arcs) == 2
        and all(
            abs(sum(edge.length for edge in run) - length) <= _length_tolerance(length)
            for run in lines
        )
        and all(
            all(
                abs(edge.radius - cylinder.radius) <= _length_tolerance(cylinder.radius)
                for edge in run
            )
            and abs(sum(edge.length for edge in run) - pi * cylinder.radius / 2)
            <= _length_tolerance(cylinder.radius)
            for run in arcs
        )
    )


def _cap_matches_profile(
    graph: FaceGraph,
    cap: frozenset[FaceNode],
    radius: float,
    flat_width: float,
) -> bool:
    wire = _region_boundary_wire(graph, cap)
    runs = _alternating_profile_runs(wire) if wire is not None else None
    if runs is None:
        return False
    lines, arcs = runs
    expected_width = flat_width + 2 * radius
    return (
        len(arcs) == 2
        and len(lines) == 2
        and all(
            all(abs(edge.radius - radius) <= _length_tolerance(radius) for edge in run)
            and abs(sum(edge.length for edge in run) - pi * radius / 2) <= _length_tolerance(radius)
            for run in arcs
        )
        and abs(min(sum(edge.length for edge in run) for run in lines) - flat_width)
        <= _length_tolerance(flat_width)
        and abs(max(sum(edge.length for edge in run) for run in lines) - expected_width)
        <= _length_tolerance(expected_width)
    )


def _common_convex_context(
    graph: FaceGraph,
    sources: tuple[frozenset[FaceNode], ...],
    normal_axis: int,
    station: float,
    nominal: float,
) -> bool:
    neighbours = {
        source: {node for member in source for node in graph.neighbours(member)}
        for source in sources
    }
    seen: set[FaceNode] = set()
    for seed in sorted(set().union(*neighbours.values()), key=lambda node: node.index):
        if seed in seen:
            continue
        region = _coplanar_region(graph, seed)
        seen.update(region)
        plane = axis_aligned_axis(graph.face(seed).wrapped)
        if (
            plane is None
            or plane[0] != normal_axis
            or abs(plane[1] - station) > _length_tolerance(nominal)
        ):
            continue
        arcs = {}
        for source in sources:
            kinds = []
            for member in source:
                for node in region & set(graph.neighbours(member)):
                    kind = graph.arc(member, node)
                    kinds.append(kind)
            arcs[source] = kinds
        if all(kinds and all(kind == "convex" for kind in kinds) for kinds in arcs.values()):
            return True
    return False


def _empty_sweep(cap_face, part, run: int, distance: float) -> bool:
    direction = [0.0, 0.0, 0.0]
    direction[run] = distance
    probe = Solid.extrude(cap_face, Vector(*direction))
    intersection = part.intersect(probe)
    return intersection_volume(intersection) == 0.0


def _recognise_one(
    solid, graph: FaceGraph
) -> list[tuple[RoundBottomBlindSlot, frozenset[FaceNode]]]:
    solid_nodes = {graph.require_node(face) for face in solid.faces()}
    bounds = solid.bounding_box()
    envelope = (
        (bounds.min.X, bounds.max.X),
        (bounds.min.Y, bounds.max.Y),
        (bounds.min.Z, bounds.max.Z),
    )
    out: list[tuple[RoundBottomBlindSlot, frozenset[FaceNode]]] = []
    seen_caps: set[FaceNode] = set()
    for cap in sorted(solid_nodes, key=lambda node: node.index):
        if cap in seen_caps:
            continue
        cap_plane = axis_aligned_axis(graph.face(cap).wrapped)
        if cap_plane is None:
            continue
        # Most planar faces are stock or belong to another family. A defining cap must touch at
        # least one of its curved walls concavely; establish that cheap AAG fact before region
        # sewing, repeated cylinder adaptation, or any Boolean work. A split cap still satisfies
        # it on each patch because each half touches one quarter cylinder.
        if not any(
            graph.surface(node) == GeomAbs_Cylinder and graph.arc(cap, node) == "concave"
            for node in graph.neighbours(cap)
        ):
            continue
        cap_region = _coplanar_region(graph, cap) & solid_nodes
        seen_caps.update(cap_region)
        if any(axis_aligned_axis(graph.face(node).wrapped) != cap_plane for node in cap_region):
            continue
        run, cap_station = cap_plane
        neighbours = {
            node
            for source in cap_region
            for node in graph.neighbours(source)
            if node not in cap_region
        }
        cylinder_by_nodes: dict[frozenset[FaceNode], _Cylinder] = {}
        planar_regions: set[frozenset[FaceNode]] = set()
        for node in neighbours:
            cylinder = _cylinder_region(graph, node)
            if cylinder is not None:
                nodes = cylinder.nodes & solid_nodes
                cylinder_by_nodes[nodes] = replace(cylinder, nodes=nodes)
            elif graph.is_planar(node):
                planar_regions.add(_coplanar_region(graph, node) & solid_nodes)
        cylinders = tuple(
            cylinder
            for nodes, cylinder in cylinder_by_nodes.items()
            if _relation(graph, cap_region, nodes) == "concave"
        )
        planar = tuple(
            region for region in planar_regions if _relation(graph, cap_region, region) == "concave"
        )
        if len(cylinders) != 2 or len(planar) != 1:
            continue
        left, right = cylinders
        floor_region = planar[0]
        floor = min(floor_region, key=lambda node: node.index)
        floor_plane = axis_aligned_axis(graph.face(floor).wrapped)
        if (
            left.axis != run
            or right.axis != run
            or floor_plane is None
            or floor_plane[0] == run
            or not _principal_rectangle(graph, floor_region, floor_plane[0])
            or abs(left.radius - right.radius) > _length_tolerance(left.radius, right.radius)
            or _relation(graph, left.nodes, floor_region) != "smooth"
            or _relation(graph, right.nodes, floor_region) != "smooth"
        ):
            continue
        depth = floor_plane[0]
        width = 3 - run - depth
        side_regions = (left.nodes, floor_region, right.nodes)
        side_nodes = tuple(node for region in side_regions for node in region)
        run_span = _same_span(graph, side_regions, run)
        if run_span is None or not all(
            _quarter_cylinder(graph, cylinder, run_span) for cylinder in (left, right)
        ):
            continue
        low, high = run_span
        run_tolerance = _length_tolerance(high - low)
        if (
            abs(cap_station - low) <= run_tolerance
            and abs(high - envelope[run][1]) <= run_tolerance
        ):
            open_sign, open_station = 1, high
        elif (
            abs(cap_station - high) <= run_tolerance
            and abs(low - envelope[run][0]) <= run_tolerance
        ):
            open_sign, open_station = -1, low
        else:
            continue
        floor_bounds = _region_bounds(graph, floor_region)
        flat_width = floor_bounds[width][1] - floor_bounds[width][0]
        radius = (left.radius + right.radius) / 2
        side_bounds = [_region_bounds(graph, left.nodes), _region_bounds(graph, right.nodes)]
        profile_low = min(item[width][0] for item in side_bounds)
        profile_high = max(item[width][1] for item in side_bounds)
        depth_low = min(item[depth][0] for item in side_bounds)
        depth_high = max(item[depth][1] for item in side_bounds)
        floor_coord = floor_plane[1]
        profile_tolerance = _length_tolerance(radius, flat_width)
        depth_open = depth_high if abs(depth_low - floor_coord) <= profile_tolerance else depth_low
        depth_sign = 1 if depth_open > floor_coord else -1
        if (
            flat_width <= 0
            or abs((profile_high - profile_low) - (flat_width + 2 * radius)) > profile_tolerance
            or abs(abs(depth_open - floor_coord) - radius) > profile_tolerance
            or not any(abs(depth_open - end) <= profile_tolerance for end in envelope[depth])
            or not _cap_matches_profile(graph, cap_region, radius, flat_width)
            or not _common_convex_context(graph, side_regions, run, open_station, high - low)
            or not _common_convex_context(
                graph,
                (left.nodes, cap_region, right.nodes),
                depth,
                depth_open,
                radius,
            )
        ):
            continue
        cap_face = _region_face(graph, cap_region)
        if cap_face is None or not _empty_sweep(cap_face, solid, run, open_station - cap_station):
            continue
        centre = [0.0, 0.0, 0.0]
        centre[run] = (low + high) / 2
        centre[width] = (profile_low + profile_high) / 2
        centre[depth] = (floor_coord + depth_open) / 2
        record = RoundBottomBlindSlot(
            axis=_AXES[run],
            open_sign=open_sign,
            length=round(high - low, 3),
            width_axis=_AXES[width],
            depth_axis=_AXES[depth],
            depth_sign=depth_sign,
            radius=round(radius, 3),
            flat_width=round(flat_width, 3),
            at=(round(centre[0], 3), round(centre[1], 3), round(centre[2], 3)),
        )
        out.append((record, frozenset((*side_nodes, *cap_region))))
    return out


def recognise_round_bottom_blind_slots(
    part: Part, *, ledger: ClaimLedger | EvidenceWriter | None = None
) -> list[RoundBottomBlindSlot]:
    """Recognise the exact analytic U-section, one-cap subset described by the record."""

    graph = ledger.graph if ledger is not None else FaceGraph(part)
    sink: EvidenceSink | None = None if ledger is None else ledger.sink
    found: list[tuple[RoundBottomBlindSlot, frozenset[FaceNode]]] = []
    for solid in part.solids():
        if not solid.is_valid:
            continue
        for record, nodes in _recognise_one(solid, graph):
            found.append((record, nodes))
    found.sort(key=lambda item: item[0])
    if sink is not None:
        # Validate the whole family before the first proposal. This preserves prefix-free
        # publication if a malformed compound exposes faces that cannot prove one closed owner.
        found = [item for item in found if graph.common_valid_solid(item[1]) is not None]
        for record, nodes in found:
            sink.propose(FamilyId.ROUND_BOTTOM_BLIND_SLOTS, record, defining=nodes)
    return [record for record, _nodes in found]
