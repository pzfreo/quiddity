# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Kernel-backed section discovery and projection; public values live in _section_recess."""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import cast

from build123d import Edge, Face, Shape, ShapeList, Solid, Vector, Wire
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Cylinder

from quiddity._adjacency import FaceGraph, FaceNode, frame_points_outward
from quiddity._geometry import length_tol
from quiddity._recess_obround import _END_RADIUS_FRAC
from quiddity._section_passages import _end_slab, _probe_prism
from quiddity._section_recess import (
    ClosedSectionProfile,
    SectionEnd,
    SectionRecessEnds,
    SectionRecessGeometry,
    Vector2,
    Vector3,
)
from quiddity._sections import (
    BodyRefIssuer,
    LocalFrame,
    PlanarSection,
    SectionEnds,
    SectionOccurrence,
    SectionVertex,
    occurrence_geometry_dict,
)
from quiddity._volume_probe import material_fraction as _material_fraction
from quiddity.passages import PassageFrame, PassageSectionVertex

_DIRECTION_TOL = 1e-6
_SEMICIRCLE_TOL = 1e-4


@dataclass(frozen=True, slots=True)
class _Candidate:
    defining_faces: tuple[int, ...]
    constituent_faces: tuple[int, ...]
    mouth: int
    body: int
    geometry: SectionRecessGeometry
    section_shape: str


def _dot(left: Vector3, right: Vector3) -> float:
    return sum(a * b for a, b in zip(left, right, strict=True))


def _subtract(left: Vector3, right: Vector3) -> Vector3:
    return cast(Vector3, tuple(a - b for a, b in zip(left, right, strict=True)))


def _scale(vector: Vector3, factor: float) -> Vector3:
    return cast(Vector3, tuple(value * factor for value in vector))


def _unit(vector: Vector3) -> Vector3 | None:
    norm = math.sqrt(_dot(vector, vector))
    return None if norm <= 1e-12 else cast(Vector3, tuple(value / norm for value in vector))


def _canonical(vector: Vector3) -> Vector3:
    normalized = _unit(vector)
    if normalized is None:
        raise ValueError("direction must be nonzero")
    value = normalized
    pivot = max(range(3), key=lambda axis: (abs(value[axis]), axis))
    if value[pivot] < 0:
        value = _scale(value, -1.0)
    return cast(Vector3, tuple(0.0 if abs(item) < 5e-13 else item for item in value))


def _parallel(left: Vector3, right: Vector3) -> bool:
    return abs(abs(_dot(left, right)) - 1.0) <= _DIRECTION_TOL


def _point(value: object) -> Vector3:
    return cast(Vector3, tuple(float(item) for item in value.Coord()))  # type: ignore[attr-defined]


def _cylinder(graph: FaceGraph, node: FaceNode) -> tuple[float, Vector3, Vector3] | None:
    surface = BRepAdaptor_Surface(graph.face(node).wrapped)
    if surface.GetType() != GeomAbs_Cylinder:
        return None
    cylinder = surface.Cylinder()
    return (
        float(cylinder.Radius()),
        _canonical(_point(cylinder.Axis().Direction())),
        _point(cylinder.Axis().Location()),
    )


def _edge_sweep(
    graph: FaceGraph, floor: FaceNode, cylinder: FaceNode, radius: float
) -> float | None:
    occurrences = graph.shared_occurrences(floor, cylinder)
    if not occurrences or any(item.edge.geom_type.name != "CIRCLE" for item in occurrences):
        return None
    return sum(float(item.edge.length) for item in occurrences) / radius


def _node_interval(
    graph: FaceGraph, node: FaceNode, direction: Vector3
) -> tuple[float, float] | None:
    values = []
    try:
        for vertex in graph.face(node).vertices():
            position = vertex.center()
            point = (float(position.X), float(position.Y), float(position.Z))
            values.append(_dot(point, direction))
    except (AttributeError, RuntimeError, TypeError, ValueError):
        return None
    return (min(values), max(values)) if values else None


def _project(point: Vector3, frame: LocalFrame) -> Vector2:
    relative = _subtract(point, frame.origin)
    return (_dot(relative, frame.u), _dot(relative, frame.v))


def _polygonal_section(
    graph: FaceGraph, floor: FaceNode, frame: LocalFrame
) -> PlanarSection | None:
    """Read one physical straight-edged floor wire into the free section frame."""

    try:
        wires = tuple(graph.face(floor).wires())
        if len(wires) != 1:
            return None
        edges = tuple(wires[0].edges())
        if len(edges) < 3 or any(edge.geom_type.name != "LINE" for edge in edges):
            return None
        points = []
        for index, edge in enumerate(edges):
            shared = tuple(
                left
                for left in edges[index - 1].vertices()
                for right in edge.vertices()
                if left == right
            )
            if len(shared) != 1:
                return None
            point = shared[0]
            points.append(_project((float(point.X), float(point.Y), float(point.Z)), frame))
        raw = PlanarSection(tuple(SectionVertex(point) for point in points))
        centre = raw.centroid
        return PlanarSection(
            tuple(
                SectionVertex((vertex.point[0] - centre[0], vertex.point[1] - centre[1]))
                for vertex in raw.boundary
            )
        )
    except (AttributeError, RuntimeError, TypeError, ValueError):
        return None


def _public_geometry(
    *,
    depth: Vector3,
    first_center: Vector3,
    second_center: Vector3,
    radius: float,
    run_interval: tuple[float, float],
    floor_at: float,
) -> SectionRecessGeometry:
    centroid = cast(
        Vector3,
        tuple(
            (left + right) / 2.0 for left, right in zip(first_center, second_center, strict=True)
        ),
    )
    frame = LocalFrame.canonical(depth, centroid)
    first = _project(first_center, frame)
    second = _project(second_center, frame)
    long = (second[0] - first[0], second[1] - first[1])
    length = math.hypot(*long)
    if length <= 1e-12:
        raise ValueError("obround cap centres must be distinct")
    direction = (long[0] / length, long[1] / length)
    width = (-direction[1], direction[0])
    points = (
        (first[0] - radius * width[0], first[1] - radius * width[1]),
        (second[0] - radius * width[0], second[1] - radius * width[1]),
        (second[0] + radius * width[0], second[1] + radius * width[1]),
        (first[0] + radius * width[0], first[1] + radius * width[1]),
    )
    section = PlanarSection(
        tuple(
            SectionVertex(point, 1.0 if index in (1, 3) else 0.0)
            for index, point in enumerate(points)
        )
    )
    issuer = BodyRefIssuer()
    occurrence = SectionOccurrence(
        issuer.issue(),
        frame,
        run_interval,
        section,
        SectionEnds(
            low_capped=abs(floor_at - run_interval[0]) <= abs(floor_at - run_interval[1]),
            high_capped=abs(floor_at - run_interval[1]) < abs(floor_at - run_interval[0]),
        ),
    )
    return project_section_recess_geometry(occurrence, body_refs=issuer)


def project_section_recess_geometry(
    occurrence: SectionOccurrence, *, body_refs: BodyRefIssuer
) -> SectionRecessGeometry:
    """Project a closed private section occurrence into the public SectionRecess geometry."""

    projected = occurrence_geometry_dict(occurrence, body_refs=body_refs)
    frame_value = cast(dict[str, list[float]], projected["frame"])
    section_value = cast(dict[str, list[dict[str, object]]], projected["section"])
    ends_value = cast(dict[str, bool], projected["ends"])
    return SectionRecessGeometry(
        "section_recess",
        PassageFrame(
            cast(Vector3, tuple(frame_value["origin"])),
            cast(Vector3, tuple(frame_value["run"])),
            cast(Vector3, tuple(frame_value["u"])),
            cast(Vector3, tuple(frame_value["v"])),
        ),
        cast(tuple[float, float], tuple(projected["run_interval"])),  # type: ignore[arg-type]
        ClosedSectionProfile(
            "closed",
            tuple(
                PassageSectionVertex(
                    cast(Vector2, tuple(cast(list[float], vertex["point"]))),
                    cast(float, vertex["bulge"]),
                )
                for vertex in section_value["boundary"]
            ),
        ),
        SectionRecessEnds(
            SectionEnd("capped" if ends_value["low_capped"] else "open"),
            SectionEnd("capped" if ends_value["high_capped"] else "open"),
        ),
    )


def _obround_prism(
    depth: Vector3,
    first: Vector3,
    second: Vector3,
    radius: float,
    low: float,
    high: float,
) -> Solid:
    """Exact line/semicircle probe, before publication rounding (not a chord polygon)."""
    along = _subtract(second, first)
    along = _subtract(along, _scale(depth, _dot(along, depth)))
    direction = _unit(along)
    if direction is None or high <= low:
        raise ValueError("obround probe requires distinct centres and increasing bounds")
    width = (
        depth[1] * direction[2] - depth[2] * direction[1],
        depth[2] * direction[0] - depth[0] * direction[2],
        depth[0] * direction[1] - depth[1] * direction[0],
    )

    def point(center: Vector3, offset: Vector3, sign: float) -> Vector:
        return Vector(
            *(
                center[i] + depth[i] * (low - _dot(center, depth)) + sign * radius * offset[i]
                for i in range(3)
            )
        )

    a, b = point(first, width, -1), point(second, width, -1)
    c, d = point(second, width, 1), point(first, width, 1)
    boundary = Wire(
        [
            Edge.make_line(a, b),
            Edge.make_three_point_arc(b, point(second, direction, 1), c),
            Edge.make_line(c, d),
            Edge.make_three_point_arc(d, point(first, direction, -1), a),
        ]
    )
    return Solid.extrude(Face(boundary), Vector(*_scale(depth, high - low)))


def _one_obround_candidate(graph: FaceGraph, floor: FaceNode) -> _Candidate | None:
    floor_normal = graph.normal(floor)
    if floor_normal is None:
        return None
    depth = _canonical(floor_normal)
    concave = tuple(node for node in graph.neighbours(floor) if graph.arc(floor, node) == "concave")
    cylinders = tuple(node for node in concave if graph.surface(node) == GeomAbs_Cylinder)
    sides = tuple(node for node in concave if graph.is_planar(node))
    if len(cylinders) != 2 or len(sides) != 2 or len(concave) != 4:
        return None
    if any(frame_points_outward(graph.face(node)) is not False for node in cylinders):
        return None
    cylinder_data = tuple(_cylinder(graph, node) for node in cylinders)
    if any(item is None for item in cylinder_data):
        return None
    first, second = (item for item in cylinder_data if item is not None)
    radius = first[0]
    if (
        abs(first[0] - second[0]) > length_tol(radius, rel=_END_RADIUS_FRAC)
        or not _parallel(first[1], second[1])
        or not _parallel(first[1], depth)
    ):
        return None
    long = _subtract(second[2], first[2])
    long = _subtract(long, _scale(depth, _dot(long, depth)))
    long_direction = _unit(long)
    if long_direction is None:
        return None
    width_direction = _canonical(
        (
            depth[1] * long_direction[2] - depth[2] * long_direction[1],
            depth[2] * long_direction[0] - depth[0] * long_direction[2],
            depth[0] * long_direction[1] - depth[1] * long_direction[0],
        )
    )
    side_normals = tuple(graph.normal(node) for node in sides)
    if any(normal is None for normal in side_normals):
        return None
    normals = tuple(_canonical(normal) for normal in side_normals if normal is not None)
    if (
        not _parallel(normals[0], normals[1])
        or any(not _parallel(normal, width_direction) for normal in normals)
        or any(abs(_dot(normal, depth)) > _DIRECTION_TOL for normal in normals)
    ):
        return None
    if not all(graph.arc(cylinder, side) == "smooth" for cylinder in cylinders for side in sides):
        return None
    sweeps = tuple(_edge_sweep(graph, floor, cylinder, radius) for cylinder in cylinders)
    if any(sweep is None or abs(sweep - math.pi) > _SEMICIRCLE_TOL for sweep in sweeps):
        return None
    intervals = tuple(_node_interval(graph, node, depth) for node in (*cylinders, *sides))
    if any(interval is None for interval in intervals):
        return None
    spans = tuple(interval for interval in intervals if interval is not None)
    low = min(interval[0] for interval in spans)
    high = max(interval[1] for interval in spans)
    tolerance = length_tol(high - low, rel=_END_RADIUS_FRAC)
    if high - low <= tolerance or any(
        abs(interval[0] - low) > tolerance or abs(interval[1] - high) > tolerance
        for interval in spans
    ):
        return None
    floor_interval = _node_interval(graph, floor, depth)
    if floor_interval is None:
        return None
    floor_at = sum(floor_interval) / 2
    if min(abs(floor_at - low), abs(floor_at - high)) > tolerance:
        return None
    mouth_at = high if abs(floor_at - low) <= tolerance else low
    context = set(graph.neighbours(cylinders[0]))
    for node in (*cylinders[1:], *sides):
        context &= set(graph.neighbours(node))
    mouths = []
    for node in context - {floor}:
        normal = graph.normal(node) if graph.is_planar(node) else None
        interval = _node_interval(graph, node, depth)
        if (
            normal is not None
            and _parallel(_canonical(normal), depth)
            and interval is not None
            and abs(sum(interval) / 2 - mouth_at) <= tolerance
            and all(
                graph.arc(node, support) in ("convex", "smooth") for support in (*cylinders, *sides)
            )
        ):
            mouths.append(node)
    owner = graph.common_valid_solid((*cylinders, *sides, floor))
    if len(mouths) != 1 or owner is None:
        return None
    try:
        solid = graph.solid_shape(owner)
        inset = 1e-6  # Same kernel-coordinate floor used by the polygonal section probes.
        if high - low <= 2 * inset:
            return None
        thickness = max(2e-5, max(1.0, high - low, radius, math.hypot(*long)) * 1e-4)
        floor_sign = -1.0 if abs(floor_at - low) <= tolerance else 1.0

        def probe(start: float, end: float) -> Solid:
            lo, hi = sorted((start, end))
            return _obround_prism(depth, first[2], second[2], radius, lo, hi)

        if (
            _material_fraction(solid, probe(low + inset, high - inset)) > 1e-9
            or _material_fraction(
                solid, probe(mouth_at - floor_sign * inset, mouth_at - floor_sign * thickness)
            )
            > 1e-9
            or _material_fraction(
                solid, probe(floor_at + floor_sign * inset, floor_at + floor_sign * thickness)
            )
            < 1.0 - 1e-9
        ):
            return None
        geometry = _public_geometry(
            depth=depth,
            first_center=first[2],
            second_center=second[2],
            radius=radius,
            run_interval=(low, high),
            floor_at=floor_at,
        )
    except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return None
    return _Candidate(
        tuple(sorted(node.index for node in (*cylinders, *sides))),
        tuple(sorted((floor.index, *(node.index for node in (*cylinders, *sides))))),
        mouths[0].index,
        owner.ordinal,
        geometry,
        "obround",
    )


def _polygonal_shape(points: tuple[Vector2, ...]) -> str:
    if len(points) == 4:
        edges = tuple(
            (points[(index + 1) % 4][0] - point[0], points[(index + 1) % 4][1] - point[1])
            for index, point in enumerate(points)
        )
        if all(
            math.hypot(*edge) > 0
            and abs(edge[0] * following[0] + edge[1] * following[1])
            <= _DIRECTION_TOL * math.hypot(*edge) * math.hypot(*following)
            for edge, following in zip(edges, (*edges[1:], edges[0]), strict=True)
        ):
            return "rectangular"
        return "polygonal"
    return {3: "triangular", 6: "hexagonal"}.get(len(points), "polygonal")


def _one_polygonal_candidate(graph: FaceGraph, floor: FaceNode) -> _Candidate | None:
    normal = graph.normal(floor)
    if normal is None:
        return None
    depth = _canonical(normal)
    walls = tuple(
        node
        for node in graph.neighbours(floor)
        if graph.arc(floor, node) == "concave" and graph.is_planar(node)
    )
    if len(walls) < 3 or any(
        (wall_normal := graph.normal(wall)) is None
        or abs(_dot(wall_normal, depth)) > _DIRECTION_TOL
        for wall in walls
    ):
        return None
    intervals = tuple(_node_interval(graph, wall, depth) for wall in walls)
    if any(interval is None for interval in intervals):
        return None
    spans = tuple(interval for interval in intervals if interval is not None)
    low = min(interval[0] for interval in spans)
    high = max(interval[1] for interval in spans)
    tolerance = length_tol(high - low, rel=_END_RADIUS_FRAC)
    if high - low <= tolerance or any(
        abs(interval[0] - low) > tolerance or abs(interval[1] - high) > tolerance
        for interval in spans
    ):
        return None
    floor_interval = _node_interval(graph, floor, depth)
    if floor_interval is None:
        return None
    floor_at = sum(floor_interval) / 2.0
    if min(abs(floor_at - low), abs(floor_at - high)) > tolerance:
        return None
    mouth_at = high if abs(floor_at - low) <= tolerance else low
    context = set(graph.neighbours(walls[0]))
    for wall in walls[1:]:
        context |= set(graph.neighbours(wall))
    mouths = []
    for node in context - {floor}:
        mouth_normal = graph.normal(node) if graph.is_planar(node) else None
        interval = _node_interval(graph, node, depth)
        if (
            mouth_normal is not None
            and _parallel(_canonical(mouth_normal), depth)
            and interval is not None
            and abs(sum(interval) / 2.0 - mouth_at) <= tolerance
            and any(graph.arc(node, wall) in ("convex", "smooth") for wall in walls)
        ):
            mouths.append(node)
    # A physical mouth may be partitioned into several coplanar stock faces. Every
    # wall still needs observed termination context on that same plane; none of
    # these consulted patches becomes defining or constituent pocket evidence.
    owner = graph.common_valid_solid((*walls, floor, *mouths))
    if (
        not mouths
        or owner is None
        or any(
            not any(graph.arc(node, wall) in ("convex", "smooth") for node in mouths)
            for wall in walls
        )
    ):
        return None
    centre = graph.face(floor).center()
    frame = LocalFrame.canonical(depth, (float(centre.X), float(centre.Y), float(centre.Z)))
    section = _polygonal_section(graph, floor, frame)
    if section is None or len(section.boundary) != len(walls):
        return None
    try:
        solid = graph.solid_shape(owner)
        scale = max(1.0, high - low)
        radius = max(math.hypot(*vertex.point) for vertex in section.boundary)
        thickness = max(2e-5, scale * 1e-4, radius * 1e-4)
        floor_sign = -1.0 if abs(floor_at - low) <= tolerance else 1.0
        mouth_sign = -floor_sign
        if (
            _material_fraction(solid, _probe_prism(frame, (low, high), section)) > 1e-9
            or _material_fraction(solid, _end_slab(frame, mouth_at, mouth_sign, thickness, section))
            > 1e-9
            or _material_fraction(solid, _end_slab(frame, floor_at, floor_sign, thickness, section))
            < 1.0 - 1e-9
        ):
            return None
    except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return None
    issuer = BodyRefIssuer()
    occurrence = SectionOccurrence(
        issuer.issue(),
        frame,
        (low, high),
        section,
        SectionEnds(
            low_capped=abs(floor_at - low) <= abs(floor_at - high),
            high_capped=abs(floor_at - high) < abs(floor_at - low),
        ),
    )
    try:
        geometry = project_section_recess_geometry(occurrence, body_refs=issuer)
    except ValueError:
        return None
    shape = _polygonal_shape(tuple(vertex.point for vertex in section.boundary))
    defining = tuple(sorted(node.index for node in walls))
    return _Candidate(
        defining,
        tuple(sorted((floor.index, *defining))),
        min(node.index for node in mouths),
        owner.ordinal,
        geometry,
        shape,
    )


def _mixed_floor_section(face: Face, depth: Vector3) -> tuple[LocalFrame, PlanarSection] | None:
    """Read the observed line/arc wire without replacing short edges or offset axes."""
    wires = tuple(face.wires())
    if len(wires) != 1:
        return None
    edges = tuple(wires[0].edges())
    kinds = {edge.geom_type.name for edge in edges}
    if kinds != {"LINE", "CIRCLE"}:
        return None
    centre = face.center()
    frame = LocalFrame.canonical(depth, (centre.X, centre.Y, centre.Z))
    corners = []
    for index, edge in enumerate(edges):
        shared = [a for a in edges[index - 1].vertices() for b in edge.vertices() if a == b]
        if len(shared) != 1:
            return None
        point = shared[0]
        corners.append(_project((point.X, point.Y, point.Z), frame))
    vertices = []
    for index, edge in enumerate(edges):
        start, end = corners[index], corners[(index + 1) % len(corners)]
        bulge = 0.0
        if edge.geom_type.name == "CIRCLE":
            midpoint = edge.position_at(0.5)
            mid = _project((midpoint.X, midpoint.Y, midpoint.Z), frame)
            cross = (mid[0] - start[0]) * (end[1] - mid[1]) - (mid[1] - start[1]) * (
                end[0] - mid[0]
            )
            if abs(cross) <= 1e-12:
                return None
            bulge = math.copysign(math.tan(edge.length / edge.radius / 4), cross)
        vertices.append(SectionVertex(start, bulge))
    raw = PlanarSection(tuple(vertices))
    offset = raw.centroid
    frame = LocalFrame.canonical(
        depth,
        cast(
            Vector3,
            tuple(
                frame.origin[i] + frame.u[i] * offset[0] + frame.v[i] * offset[1] for i in range(3)
            ),
        ),
    )
    section = PlanarSection(
        tuple(
            SectionVertex((vertex.point[0] - offset[0], vertex.point[1] - offset[1]), vertex.bulge)
            for vertex in raw.boundary
        )
    )
    return frame, section


def _covered_patch(patch: Face, supports: tuple[Face, ...]) -> bool:
    """Prove full physical support, counting split faces by union rather than summed area."""
    remaining: list[Shape] = [patch]
    for support in supports:
        fragments: list[Shape] = []
        for fragment in remaining:
            difference = fragment.cut(support)
            if isinstance(difference, ShapeList):
                fragments.extend(difference)
            elif difference is not None:
                fragments.append(difference)
        remaining = fragments
        if sum(fragment.area for fragment in remaining) <= patch.area * 1e-9:
            return True
    return False


def _one_mixed_candidate(graph: FaceGraph, floor: FaceNode) -> _Candidate | None:
    """Prove an intact native mixed line/arc pocket from its exact swept floor."""
    normal = graph.normal(floor)
    if normal is None:
        return None
    depth = _canonical(normal)
    try:
        source = graph.face(floor)
        profile = _mixed_floor_section(source, depth)
        if profile is None:
            return None
        frame, section = profile
        walls = tuple(
            node for node in graph.neighbours(floor) if graph.arc(floor, node) == "concave"
        )
        spans = tuple(_node_interval(graph, wall, depth) for wall in walls)
        if not walls or any(span is None for span in spans):
            return None
        low = min(span[0] for span in spans if span is not None)
        high = max(span[1] for span in spans if span is not None)
        floor_at = _dot((source.center().X, source.center().Y, source.center().Z), depth)
        tolerance = 1e-6
        if high - low <= 2 * tolerance:
            return None
        if abs(floor_at - low) <= tolerance:
            mouth_at, sign = high, 1.0
        elif abs(floor_at - high) <= tolerance:
            mouth_at, sign = low, -1.0
        else:
            return None
        context = {node for wall in walls for node in graph.neighbours(wall)} - {floor}
        mouths = tuple(
            node
            for node in context
            if graph.is_planar(node)
            and (n := graph.normal(node)) is not None
            and _parallel(n, depth)
            and (span := _node_interval(graph, node, depth)) is not None
            and max(abs(value - mouth_at) for value in span) <= tolerance
        )
        if not mouths or any(
            not any(graph.arc(wall, mouth) in ("convex", "smooth") for mouth in mouths)
            for wall in walls
        ):
            return None
        owner = graph.common_valid_solid((floor, *walls, *mouths))
        if owner is None:
            return None

        def probe(start: float, end: float) -> Solid:
            base = source.translate(Vector(*_scale(depth, start - floor_at)))
            return Solid.extrude(base, Vector(*_scale(depth, end - start)))

        swept = probe(floor_at, mouth_at)
        supports = tuple(graph.face(wall) for wall in walls)
        # Include caps in the subtraction proof too: this avoids relying on generated
        # face ordering or guessing which extrusion faces are the lateral supports.
        cap = source.translate(Vector(*_scale(depth, mouth_at - floor_at)))
        if any(not _covered_patch(patch, (*supports, source, cap)) for patch in swept.faces()):
            return None
        solid = graph.solid_shape(owner)
        thickness = max(2e-5, max(1.0, high - low, math.sqrt(source.area)) * 1e-4)
        if (
            _material_fraction(solid, probe(low + tolerance, high - tolerance)) > 1e-9
            or _material_fraction(
                solid, probe(mouth_at + sign * tolerance, mouth_at + sign * thickness)
            )
            > 1e-9
            or _material_fraction(
                solid, probe(floor_at - sign * tolerance, floor_at - sign * thickness)
            )
            < 1 - 1e-9
        ):
            return None
        issuer = BodyRefIssuer()
        geometry = project_section_recess_geometry(
            SectionOccurrence(
                issuer.issue(), frame, (low, high), section, SectionEnds(sign > 0, sign < 0)
            ),
            body_refs=issuer,
        )
        defining = tuple(sorted(node.index for node in walls))
        return _Candidate(
            defining,
            tuple(sorted((floor.index, *defining))),
            min(node.index for node in mouths),
            owner.ordinal,
            geometry,
            "general",
        )
    except (AttributeError, RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return None


def _candidates(graph: FaceGraph) -> tuple[_Candidate, ...]:
    found = set()
    for node in graph.nodes:
        if not graph.is_planar(node):
            continue
        # Prefer the existing proved specific classification. The general reader
        # is a fallback for this floor, not a second occurrence of the same pocket.
        candidate = (
            _one_obround_candidate(graph, node)
            or _one_polygonal_candidate(graph, node)
            or _one_mixed_candidate(graph, node)
        )
        if candidate is not None:
            found.add(candidate)
    return tuple(
        sorted(found, key=lambda item: (item.constituent_faces, item.section_shape, item.mouth))
    )


__all__ = ["project_section_recess_geometry"]
