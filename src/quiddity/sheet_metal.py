# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Developable, two-sided sheet bodies with flange and bend evidence.

The neutral-axis allowance uses an explicit k-factor. The material and its
forming history are absent from a STEP solid, so neither is inferred here.
Face indices refer to the supplied part's face roster for this one run.
"""

from __future__ import annotations

import math
from dataclasses import dataclass, replace

from build123d import Edge, Face
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Cylinder, GeomAbs_Plane

from quiddity._adjacency import FaceGraph
from quiddity._candidates import CompletedInputs, FamilyId
from quiddity._definitions import (
    DiscoveryServices,
    FullyAttributed,
    ManifestEvidence,
    NotCounted,
    PhysicalDefinition,
    always,
)
from quiddity._geometry import length_tol
from quiddity._record import Record
from quiddity._typing import Part
from quiddity.thin_walls import ThinWallBody, WallFacePair, _discover_thin_wall_bodies

_MIN_SKIN_COVERAGE = 0.98
_MIN_CUT_BRIDGE_AREA = 0.8
_DEFAULT_K_FACTOR = 0.5


@dataclass(frozen=True, slots=True)
class SheetFlange(Record):
    """One planar region on the reference side and its opposite skin patches."""

    index: int
    reference_faces: tuple[int, ...]
    mate_faces: tuple[int, ...]
    origin: tuple[float, float, float]
    normal: tuple[float, float, float]
    area: float


@dataclass(frozen=True, slots=True)
class SheetBend(Record):
    """A cylindrical bend pair between two flanges."""

    index: int
    face_pairs: tuple[WallFacePair, ...]
    first_flange: int
    second_flange: int
    axis_origin: tuple[float, float, float]
    axis_direction: tuple[float, float, float]
    angle_degrees: float
    inner_radius: float
    reference_skin: str
    neutral_radius: float
    bend_allowance: float


@dataclass(frozen=True, slots=True)
class UnfoldedFlangeFace(Record):
    """Triangulated planar source face in the neutral flat coordinate frame."""

    source_face: int
    flange: int
    vertices: tuple[tuple[float, float], ...]
    triangles: tuple[tuple[int, int, int], ...]


@dataclass(frozen=True, slots=True)
class UnfoldedBendStrip(Record):
    """Neutral-axis developed rectangle for one cylindrical bend segment."""

    bend: int
    source_face_pair: WallFacePair
    corners: tuple[
        tuple[float, float],
        tuple[float, float],
        tuple[float, float],
        tuple[float, float],
    ]


@dataclass(frozen=True, slots=True)
class FlatPatternPlan(Record):
    """A checked neutral flat layout of flange faces and developed bend strips.

    Triangulated flange faces preserve boundary cutouts from the source B-rep. A
    formed feature remains a separate child, not a manufacturing blank claim.
    """

    k_factor: float
    base_flange: int
    tree_bends: tuple[int, ...]
    flat_faces: tuple[UnfoldedFlangeFace, ...]
    bend_strips: tuple[UnfoldedBendStrip, ...]
    tessellation_tolerance: float


@dataclass(frozen=True, slots=True)
class FormedSheetFeature(Record):
    """A local curved region with partial offset-skin evidence."""

    faces: tuple[int, ...]
    paired_faces: tuple[int, ...]


@dataclass(frozen=True, slots=True)
class SheetMetalBody(Record):
    """A dominant developable sheet with explicit flange and bend geometry."""

    body_index: int
    body_key: tuple[float, ...] | None
    thickness: float
    first_side_faces: tuple[int, ...]
    second_side_faces: tuple[int, ...]
    cut_edge_faces: tuple[int, ...]
    formed_features: tuple[FormedSheetFeature, ...]
    flanges: tuple[SheetFlange, ...]
    bends: tuple[SheetBend, ...]
    flat_pattern: FlatPatternPlan
    paired_area_fraction: float


def _surface(face: Face) -> BRepAdaptor_Surface:
    return BRepAdaptor_Surface(face.wrapped)


def _coordinates(point: object) -> tuple[float, float, float]:
    return (float(point.X()), float(point.Y()), float(point.Z()))  # type: ignore[attr-defined]


def _same_axis(
    first_origin: tuple[float, float, float],
    first_direction: tuple[float, float, float],
    second_origin: tuple[float, float, float],
    second_direction: tuple[float, float, float],
    tolerance: float,
) -> bool:
    if (
        abs(abs(sum(a * b for a, b in zip(first_direction, second_direction, strict=True))) - 1)
        > 1e-5
    ):
        return False
    along = sum((second_origin[i] - first_origin[i]) * first_direction[i] for i in range(3))
    projected = tuple(first_origin[i] + along * first_direction[i] for i in range(3))
    return math.dist(projected, second_origin) <= tolerance


def _sides(
    wall: ThinWallBody, graph: FaceGraph, faces: tuple[Face, ...]
) -> tuple[set[int], set[int]] | None:
    plane_pairs = [
        pair
        for pair in wall.face_pairs
        if _surface(faces[pair.first_face]).GetType() == GeomAbs_Plane
    ]
    if len(plane_pairs) < 2:
        return None
    seed = max(
        plane_pairs,
        key=lambda pair: faces[pair.first_face].area + faces[pair.second_face].area,
    )
    indices = {face: index for index, face in enumerate(faces)}
    first = {
        indices[graph.face(node)]
        for node in graph.smooth_region(graph.require_node(faces[seed.first_face]))
    }
    second = {
        indices[graph.face(node)]
        for node in graph.smooth_region(graph.require_node(faces[seed.second_face]))
    }
    if first & second:
        return None
    paired = {index for pair in wall.face_pairs for index in (pair.first_face, pair.second_face)}
    coverage = math.fsum(faces[index].area for index in (first | second) & paired) / math.fsum(
        faces[index].area for index in paired
    )
    return (first, second) if coverage >= _MIN_SKIN_COVERAGE else None


def _cut_faces(
    wall: ThinWallBody,
    graph: FaceGraph,
    faces: tuple[Face, ...],
    first: set[int],
    second: set[int],
) -> tuple[tuple[int, ...], tuple[int, ...]] | None:
    body_faces = {
        index for pair in wall.face_pairs for index in (pair.first_face, pair.second_face)
    } | set(wall.unpaired_faces)
    remaining = body_faces - first - second
    indices = {face: index for index, face in enumerate(faces)}
    cut = []
    for index in sorted(remaining):
        node = graph.require_node(faces[index])
        first_edges: list[Edge] = []
        second_edges: list[Edge] = []
        for neighbour in graph.neighbours(node):
            neighbour_index = indices[graph.face(neighbour)]
            if neighbour_index in first:
                first_edges.extend(graph.shared_edges(node, neighbour))
            if neighbour_index in second:
                second_edges.extend(graph.shared_edges(node, neighbour))
        if first_edges and second_edges:
            tolerance = length_tol(wall.thickness, rel=0.01)
            for source, opposite in ((first_edges, second_edges), (second_edges, first_edges)):
                for edge in source:
                    for fraction in (0.2, 0.5, 0.8):
                        distance = min(
                            other.distance_to(edge.position_at(fraction)) for other in opposite
                        )
                        if abs(distance - wall.thickness) > tolerance:
                            return None
            cut.append(index)
    total = math.fsum(faces[index].area for index in remaining)
    bridged = math.fsum(faces[index].area for index in cut)
    if total <= 0 or bridged / total < _MIN_CUT_BRIDGE_AREA:
        return None
    return tuple(cut), tuple(sorted(remaining - set(cut)))


def _formed_features(
    unresolved: tuple[int, ...],
    wall: ThinWallBody,
    graph: FaceGraph,
    faces: tuple[Face, ...],
) -> tuple[FormedSheetFeature, ...] | None:
    """Refuse unexplained regions unless curved local forming retains paired skins."""

    remaining = set(unresolved)
    paired = {index for pair in wall.face_pairs for index in (pair.first_face, pair.second_face)}
    indices = {face: index for index, face in enumerate(faces)}
    result = []
    mates = {
        index: other
        for pair in wall.face_pairs
        for index, other in (
            (pair.first_face, pair.second_face),
            (pair.second_face, pair.first_face),
        )
    }
    while remaining:
        seed = min(remaining)
        remaining.remove(seed)
        component = {seed}
        pending = [seed]
        while pending:
            node = graph.require_node(faces[pending.pop()])
            for neighbour in graph.neighbours(node):
                index = indices[graph.face(neighbour)]
                if index in remaining:
                    remaining.remove(index)
                    component.add(index)
                    pending.append(index)
        supported = component & paired
        if not supported or not any(
            _surface(faces[index]).GetType() == GeomAbs_Cylinder for index in supported
        ):
            return None
        formed_faces = component | {mates[index] for index in supported}
        result.append(FormedSheetFeature(tuple(sorted(formed_faces)), tuple(sorted(supported))))
    return tuple(result)


def _flanges(
    wall: ThinWallBody,
    graph: FaceGraph,
    faces: tuple[Face, ...],
    first: set[int],
    second: set[int],
    excluded: set[int],
) -> tuple[tuple[SheetFlange, ...], dict[int, int]] | None:
    mates: dict[int, set[int]] = {}
    for pair in wall.face_pairs:
        if pair.first_face in excluded or pair.second_face in excluded:
            continue
        if _surface(faces[pair.first_face]).GetType() != GeomAbs_Plane:
            continue
        a, b = pair.first_face, pair.second_face
        if a in first and b not in first:
            reference, mate = a, b
        elif (b in first and a not in first) or (a in second and b not in second):
            reference, mate = b, a
        elif b in second and a not in second:
            reference, mate = a, b
        else:
            return None
        mates.setdefault(reference, set()).add(mate)
    flanges: list[SheetFlange] = []
    face_to_flange: dict[int, int] = {}
    indices = {face: index for index, face in enumerate(faces)}
    axes: dict[int, tuple[tuple[tuple[float, float, float], tuple[float, float, float]], ...]] = {}
    for reference in mates:
        adjacent_axes = []
        for neighbour in graph.neighbours(graph.require_node(faces[reference])):
            index = indices[graph.face(neighbour)]
            surface = _surface(faces[index])
            if surface.GetType() == GeomAbs_Cylinder:
                axis = surface.Cylinder().Axis()
                adjacent_axes.append(
                    (_coordinates(axis.Location()), _coordinates(axis.Direction()))
                )
        axes[reference] = tuple(adjacent_axes)
    for reference in sorted(mates):
        surface = _surface(faces[reference]).Plane()
        origin = _coordinates(surface.Location())
        direction = faces[reference].normal_at()
        normal = (direction.X, direction.Y, direction.Z)
        group = next(
            (
                index
                for index, flange in enumerate(flanges)
                if math.dist(normal, flange.normal) < 1e-5
                and abs(
                    sum(n * (a - b) for n, a, b in zip(normal, origin, flange.origin, strict=True))
                )
                <= length_tol(wall.thickness, rel=0.001)
                and any(
                    _same_axis(*left, *right, length_tol(wall.thickness, rel=0.001))
                    for existing in flange.reference_faces
                    for left in axes[reference]
                    for right in axes[existing]
                )
            ),
            None,
        )
        if group is None:
            group = len(flanges)
            flanges.append(
                SheetFlange(
                    index=group,
                    reference_faces=(reference,),
                    mate_faces=tuple(sorted(mates[reference])),
                    origin=origin,
                    normal=normal,
                    area=faces[reference].area,
                )
            )
        else:
            prior = flanges[group]
            flanges[group] = replace(
                prior,
                reference_faces=(*prior.reference_faces, reference),
                mate_faces=tuple(sorted((*prior.mate_faces, *mates[reference]))),
                area=prior.area + faces[reference].area,
            )
        face_to_flange[reference] = group
    return tuple(flanges), face_to_flange


def _bends(
    wall: ThinWallBody,
    graph: FaceGraph,
    faces: tuple[Face, ...],
    first: set[int],
    face_to_flange: dict[int, int],
    k_factor: float,
    excluded: set[int],
) -> tuple[SheetBend, ...] | None:
    indices = {face: index for index, face in enumerate(faces)}
    result: list[SheetBend] = []
    for pair in wall.face_pairs:
        if pair.first_face in excluded or pair.second_face in excluded:
            continue
        surface_a = _surface(faces[pair.first_face])
        if surface_a.GetType() != GeomAbs_Cylinder:
            continue
        surface_b = _surface(faces[pair.second_face])
        if surface_b.GetType() != GeomAbs_Cylinder:
            return None
        radius_a = float(surface_a.Cylinder().Radius())
        radius_b = float(surface_b.Cylinder().Radius())
        if abs(abs(radius_a - radius_b) - wall.thickness) > length_tol(wall.thickness, rel=0.001):
            return None
        reference = pair.first_face if pair.first_face in first else pair.second_face
        if reference not in first:
            return None
        reference_surface = surface_a if reference == pair.first_face else surface_b
        # Relief cuts can trim the opposite skin to a shorter angular span.
        # The continuous reference-side patch retains the bend's full sweep.
        angle = abs(reference_surface.LastUParameter() - reference_surface.FirstUParameter())
        if not 0 < angle < 2 * math.pi - 1e-3:
            return None
        neighbours = {
            indices[graph.face(node)]
            for node in graph.neighbours(graph.require_node(faces[reference]))
        }
        adjacent = sorted({face_to_flange[i] for i in neighbours if i in face_to_flange})
        if len(adjacent) != 2:
            return None
        cylinder = surface_a.Cylinder()
        axis = cylinder.Axis()
        neutral_radius = min(radius_a, radius_b) + k_factor * wall.thickness
        reference_radius = radius_a if reference == pair.first_face else radius_b
        reference_skin = (
            "inner"
            if abs(reference_radius - min(radius_a, radius_b))
            <= length_tol(wall.thickness, rel=0.001)
            else "outer"
        )
        origin = _coordinates(axis.Location())
        direction = _coordinates(axis.Direction())
        match = next(
            (
                index
                for index, bend in enumerate(result)
                if (bend.first_flange, bend.second_flange) == tuple(adjacent)
                and abs(bend.angle_degrees - math.degrees(angle)) < 0.01
                and abs(bend.inner_radius - min(radius_a, radius_b))
                <= length_tol(wall.thickness, rel=0.001)
                and bend.reference_skin == reference_skin
                and _same_axis(
                    origin,
                    direction,
                    bend.axis_origin,
                    bend.axis_direction,
                    length_tol(wall.thickness, rel=0.001),
                )
            ),
            None,
        )
        if match is not None:
            result[match] = replace(result[match], face_pairs=(*result[match].face_pairs, pair))
            continue
        result.append(
            SheetBend(
                index=len(result),
                face_pairs=(pair,),
                first_flange=adjacent[0],
                second_flange=adjacent[1],
                axis_origin=origin,
                axis_direction=direction,
                angle_degrees=math.degrees(angle),
                inner_radius=min(radius_a, radius_b),
                reference_skin=reference_skin,
                neutral_radius=neutral_radius,
                bend_allowance=neutral_radius * angle,
            )
        )
    return tuple(result) if result else None


Vec3 = tuple[float, float, float]
Vec2 = tuple[float, float]
Matrix3 = tuple[Vec3, Vec3, Vec3]
_IDENTITY: Matrix3 = ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))


def _dot(a: Vec3, b: Vec3) -> float:
    return sum(x * y for x, y in zip(a, b, strict=True))


def _sub(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] - b[0], a[1] - b[1], a[2] - b[2])


def _add(a: Vec3, b: Vec3) -> Vec3:
    return (a[0] + b[0], a[1] + b[1], a[2] + b[2])


def _scale(a: Vec3, factor: float) -> Vec3:
    return (a[0] * factor, a[1] * factor, a[2] * factor)


def _cross(a: Vec3, b: Vec3) -> Vec3:
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _unit(a: Vec3) -> Vec3 | None:
    size = math.sqrt(_dot(a, a))
    return _scale(a, 1 / size) if size > 1e-9 else None


def _matrix_vector(matrix: Matrix3, vector: Vec3) -> Vec3:
    return (_dot(matrix[0], vector), _dot(matrix[1], vector), _dot(matrix[2], vector))


def _matrix_product(left: Matrix3, right: Matrix3) -> Matrix3:
    return tuple(  # type: ignore[return-value]
        tuple(sum(left[i][k] * right[k][j] for k in range(3)) for j in range(3)) for i in range(3)
    )


def _rotation(axis: Vec3, angle: float) -> Matrix3:
    x, y, z = axis
    c, s = math.cos(angle), math.sin(angle)
    t = 1 - c
    return (
        (t * x * x + c, t * x * y - s * z, t * x * z + s * y),
        (t * x * y + s * z, t * y * y + c, t * y * z - s * x),
        (t * x * z - s * y, t * y * z + s * x, t * z * z + c),
    )


def _vector(point: object) -> Vec3:
    return (float(point.X), float(point.Y), float(point.Z))  # type: ignore[attr-defined]


def _cross2(a: Vec2, b: Vec2) -> float:
    return a[0] * b[1] - a[1] * b[0]


def _area2(polygon: list[Vec2]) -> float:
    return 0.5 * sum(
        _cross2(polygon[i], polygon[(i + 1) % len(polygon)]) for i in range(len(polygon))
    )


def _overlap_area(first: tuple[Vec2, Vec2, Vec2], second: tuple[Vec2, Vec2, Vec2]) -> float:
    """Area of two clipped triangles; shared boundaries have zero overlap."""

    subject = list(first)
    clip = list(second)
    if _area2(clip) < 0:
        clip.reverse()
    for i in range(3):
        start, end = clip[i], clip[(i + 1) % 3]
        edge = (end[0] - start[0], end[1] - start[1])
        output: list[Vec2] = []
        if not subject:
            return 0.0
        previous = subject[-1]
        previous_side = _cross2(edge, (previous[0] - start[0], previous[1] - start[1]))
        for point in subject:
            side = _cross2(edge, (point[0] - start[0], point[1] - start[1]))
            if (side >= 0) != (previous_side >= 0):
                fraction = previous_side / (previous_side - side)
                output.append(
                    (
                        previous[0] + fraction * (point[0] - previous[0]),
                        previous[1] + fraction * (point[1] - previous[1]),
                    )
                )
            if side >= 0:
                output.append(point)
            previous, previous_side = point, side
        subject = output
    return abs(_area2(subject)) if len(subject) >= 3 else 0.0


def _joint_edges(
    bend: SheetBend,
    parent: SheetFlange,
    child: SheetFlange,
    first: set[int],
    graph: FaceGraph,
    faces: tuple[Face, ...],
) -> tuple[int, Edge, Edge] | None:
    for pair in bend.face_pairs:
        reference = pair.first_face if pair.first_face in first else pair.second_face
        node = graph.require_node(faces[reference])
        parent_edges = [
            edge
            for index in parent.reference_faces
            for edge in graph.shared_edges(node, graph.require_node(faces[index]))
        ]
        child_edges = [
            edge
            for index in child.reference_faces
            for edge in graph.shared_edges(node, graph.require_node(faces[index]))
        ]
        if parent_edges and child_edges:
            return (
                reference,
                max(parent_edges, key=lambda edge: edge.length),
                max(child_edges, key=lambda edge: edge.length),
            )
    return None


def _flat_pattern_plan(
    flanges: tuple[SheetFlange, ...],
    bends: tuple[SheetBend, ...],
    k_factor: float,
    thickness: float,
    first: set[int],
    graph: FaceGraph,
    faces: tuple[Face, ...],
) -> FlatPatternPlan | None:
    if not flanges or len(bends) != len(flanges) - 1:
        return None
    base = max(flanges, key=lambda flange: flange.area).index
    root = flanges[base]
    root_normal = root.normal
    horizontal = (0.0, 0.0, 1.0) if abs(root_normal[2]) < 0.9 else (1.0, 0.0, 0.0)
    x_axis = _unit(_cross(horizontal, root_normal))
    if x_axis is None:
        return None
    y_axis = _cross(root_normal, x_axis)
    placement: dict[int, tuple[Matrix3, Vec3]] = {base: (_IDENTITY, (0.0, 0.0, 0.0))}
    tree: list[int] = []
    pending = [base]
    while pending:
        current = pending.pop(0)
        for bend in bends:
            other = (
                bend.second_flange
                if bend.first_flange == current
                else bend.first_flange
                if bend.second_flange == current
                else None
            )
            if other is None or other in placement:
                continue
            parent, child = flanges[current], flanges[other]
            joint = _joint_edges(bend, parent, child, first, graph, faces)
            if joint is None:
                return None
            reference, parent_edge, child_edge = joint
            parent_point = _vector(parent_edge.position_at(0.5))
            child_point = _vector(child_edge.position_at(0.5))
            cylinder_center = _vector(faces[reference].center())
            approach = _sub(cylinder_center, parent_point)
            approach = _sub(approach, _scale(parent.normal, _dot(approach, parent.normal)))
            unit_approach = _unit(approach)
            if unit_approach is None:
                return None
            axis = bend.axis_direction
            angle = math.atan2(
                _dot(axis, _cross(child.normal, parent.normal)),
                _dot(child.normal, parent.normal),
            )
            rotation = _rotation(axis, angle)
            target = _add(parent_point, _scale(unit_approach, bend.bend_allowance))
            shift = _sub(target, _matrix_vector(rotation, child_point))
            parent_matrix, parent_shift = placement[current]
            placement[other] = (
                _matrix_product(parent_matrix, rotation),
                _add(_matrix_vector(parent_matrix, shift), parent_shift),
            )
            tree.append(bend.index)
            pending.append(other)
    if len(placement) != len(flanges):
        return None

    def flat(point: Vec3, flange: int) -> Vec2:
        matrix, shift = placement[flange]
        placed = _sub(_add(_matrix_vector(matrix, point), shift), root.origin)
        return (_dot(placed, x_axis), _dot(placed, y_axis))

    flat_faces: list[UnfoldedFlangeFace] = []
    triangles: list[tuple[int, tuple[Vec2, Vec2, Vec2]]] = []
    for flange in flanges:
        for face_index in flange.reference_faces:
            vertices, cells = faces[face_index].tessellate(0.1)
            projected = tuple(flat(_vector(vertex), flange.index) for vertex in vertices)
            flat_faces.append(UnfoldedFlangeFace(face_index, flange.index, projected, tuple(cells)))
            triangles.extend(
                (face_index, (projected[a], projected[b], projected[c])) for a, b, c in cells
            )
    strips: list[UnfoldedBendStrip] = []
    for bend in bends:
        parent = flanges[bend.first_flange]
        child = flanges[bend.second_flange]
        for pair in bend.face_pairs:
            segment = replace(bend, face_pairs=(pair,))
            joint = _joint_edges(segment, parent, child, first, graph, faces)
            if joint is None:
                return None
            reference, edge, _ = joint
            p0 = _vector(edge.position_at(0))
            p1 = _vector(edge.position_at(1))
            middle = _vector(faces[reference].center())
            approach = _sub(middle, _vector(edge.position_at(0.5)))
            approach = _sub(approach, _scale(parent.normal, _dot(approach, parent.normal)))
            unit_approach = _unit(approach)
            if unit_approach is None:
                return None
            q0 = _add(p0, _scale(unit_approach, bend.bend_allowance))
            q1 = _add(p1, _scale(unit_approach, bend.bend_allowance))
            corners = (
                flat(p0, parent.index),
                flat(p1, parent.index),
                flat(q1, parent.index),
                flat(q0, parent.index),
            )
            strips.append(UnfoldedBendStrip(bend.index, pair, corners))
            triangles.append((-(len(strips)), (corners[0], corners[1], corners[2])))
            triangles.append((-(len(strips)), (corners[0], corners[2], corners[3])))
    for i, (first_id, first_triangle) in enumerate(triangles):
        first_x = [point[0] for point in first_triangle]
        first_y = [point[1] for point in first_triangle]
        for second_id, second_triangle in triangles[i + 1 :]:
            if first_id == second_id:
                continue
            if max(first_x) <= min(point[0] for point in second_triangle) + 1e-6:
                continue
            if max(point[0] for point in second_triangle) <= min(first_x) + 1e-6:
                continue
            if max(first_y) <= min(point[1] for point in second_triangle) + 1e-6:
                continue
            if max(point[1] for point in second_triangle) <= min(first_y) + 1e-6:
                continue
            if _overlap_area(first_triangle, second_triangle) > 0.01 * thickness**2:
                return None
    return FlatPatternPlan(k_factor, base, tuple(tree), tuple(flat_faces), tuple(strips), 0.1)


def _body(
    wall: ThinWallBody,
    graph: FaceGraph,
    faces: tuple[Face, ...],
    k_factor: float,
) -> SheetMetalBody | None:
    skin_types = {
        _surface(faces[index]).GetType()
        for pair in wall.face_pairs
        for index in (pair.first_face, pair.second_face)
    }
    if not skin_types <= {GeomAbs_Plane, GeomAbs_Cylinder} or GeomAbs_Cylinder not in skin_types:
        return None
    if any(
        _surface(faces[index]).GetType() not in {GeomAbs_Plane, GeomAbs_Cylinder}
        for index in wall.unpaired_faces
    ):
        return None
    sides = _sides(wall, graph, faces)
    if sides is None:
        return None
    first, second = sides
    cut = _cut_faces(wall, graph, faces, first, second)
    if cut is None:
        return None
    cut_faces, unresolved = cut
    formed_features = _formed_features(unresolved, wall, graph, faces)
    if formed_features is None:
        return None
    excluded = {index for feature in formed_features for index in feature.faces}
    flange_result = _flanges(wall, graph, faces, first, second, excluded)
    if flange_result is None:
        return None
    flanges, face_to_flange = flange_result
    bends = _bends(wall, graph, faces, first, face_to_flange, k_factor, excluded)
    if bends is None:
        return None
    plan = _flat_pattern_plan(flanges, bends, k_factor, wall.thickness, first, graph, faces)
    if plan is None:
        return None
    return SheetMetalBody(
        body_index=wall.body_index,
        body_key=wall.body_key,
        thickness=wall.thickness,
        first_side_faces=tuple(sorted(first - excluded)),
        second_side_faces=tuple(sorted(second - excluded)),
        cut_edge_faces=cut_faces,
        formed_features=formed_features,
        flanges=flanges,
        bends=bends,
        flat_pattern=plan,
        paired_area_fraction=wall.paired_area_fraction,
    )


def recognise_sheet_metal_bodies(
    part: Part, *, k_factor: float = _DEFAULT_K_FACTOR
) -> list[SheetMetalBody]:
    """Recognise developable folded sheets; use *k_factor* for bend allowances."""

    if not math.isfinite(k_factor) or not 0 <= k_factor <= 1:
        raise ValueError("k_factor must be finite and between zero and one")
    graph = FaceGraph(part)
    faces = tuple(part.faces())
    return _build_sheet_metal_records(
        _discover_thin_wall_bodies(part, graph=graph), graph, faces, k_factor
    )


def _build_sheet_metal_records(
    walls: list[ThinWallBody] | tuple[ThinWallBody, ...],
    graph: FaceGraph,
    faces: tuple[Face, ...],
    k_factor: float,
) -> list[SheetMetalBody]:
    return [record for wall in walls if (record := _body(wall, graph, faces, k_factor)) is not None]


def _discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    faces = tuple(services.context.part.faces())
    graph = services.context.graph
    records = _build_sheet_metal_records(
        inputs.records(FamilyId.THIN_WALL_BODIES, ThinWallBody),
        graph,
        faces,
        _DEFAULT_K_FACTOR,
    )
    for record in records:
        indices = set(record.first_side_faces) | set(record.second_side_faces)
        indices.update(record.cut_edge_faces)
        indices.update(index for feature in record.formed_features for index in feature.faces)
        services.writer.add_defining(
            record,
            (graph.require_node(faces[index]) for index in sorted(indices)),
            family=FamilyId.SHEET_METAL_BODIES,
        )
    return list(records)


DEFINITION = PhysicalDefinition(
    family=FamilyId.SHEET_METAL_BODIES,
    record_types=(SheetMetalBody,),
    result_field="sheet_metal_bodies",
    public_entrypoint=recognise_sheet_metal_bodies.__name__,
    dependencies=(FamilyId.THIN_WALL_BODIES,),
    applicable=always,
    discover=_discover,
    census=NotCounted("whole-body fabrication interpretation, not a machined feature count"),
    attribution=FullyAttributed("accepted sheets claim both propagated skins and cut-edge faces"),
    evidence=ManifestEvidence(
        tests=("tests/test_sheet_metal.py",),
        golden_paths=("tests/sheet_metal_expected.json",),
        introduced="0.3.4",
        extra_records=(
            ("SheetFlange", "nested", ()),
            ("SheetBend", "nested", ()),
            ("FlatPatternPlan", "nested", ()),
            ("FormedSheetFeature", "nested", ()),
            ("UnfoldedFlangeFace", "nested", ()),
            ("UnfoldedBendStrip", "nested", ()),
        ),
    ),
)
