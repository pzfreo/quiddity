# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Original-source proof of a three-support channel ending on a native bore."""

from __future__ import annotations

import math
from dataclasses import dataclass
from itertools import product
from typing import cast

from build123d import Compound, Face, GeomType, Plane, Shape, ShapeList, Solid, Vector, Wire

from quiddity._adjacency import FaceGraph, FaceNode, SolidRef
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact,
    EffectiveSurfaceQuery,
    SurfaceKind,
    SurfaceProvenance,
)
from quiddity._support_patches import covered_patch
from quiddity._volume_probe import material_fraction

Vector3 = tuple[float, float, float]
Bounds3 = tuple[tuple[float, float], tuple[float, float], tuple[float, float]]


@dataclass(frozen=True, slots=True)
class CylindricalChannelProof:
    supports: tuple[FaceNode, ...]
    cylinder: FaceNode
    planar_context: FaceNode
    owner: SolidRef
    run_axis: str
    width_axis: str
    open_sign: int
    bounds: Bounds3
    run_interval: tuple[float, float]
    cylindrical_end: int
    axis_point: Vector3
    axis_direction: Vector3
    radius: float
    volume: float


def _shape(value: Shape | ShapeList | None) -> Shape:
    return (
        Compound([])
        if value is None
        else Compound(value)
        if isinstance(value, ShapeList)
        else value
    )


def _bounds(face: Face) -> Bounds3:
    # Include extrema inside curved trims, not only their endpoint vertices.
    box = face.bounding_box()
    return cast(Bounds3, tuple(zip(tuple(box.min), tuple(box.max), strict=True)))


def _at(face: Face, axis: int, position: float) -> bool:
    return max(abs(value - position) for value in _bounds(face)[axis]) <= 1e-6


def _rectangle(bounds: Bounds3, axis: int, at: float) -> Face:
    transverse = [i for i in range(3) if i != axis]
    corners = list(product(*(bounds[i] for i in transverse)))
    points = []
    for first, second in (corners[0], corners[1], corners[3], corners[2]):
        point = [0.0, 0.0, 0.0]
        point[axis], point[transverse[0]], point[transverse[1]] = at, first, second
        points.append(tuple(point))
    return Face(Wire.make_polygon(points, close=True))


def _prove(
    graph: FaceGraph,
    surfaces: EffectiveSurfaceQuery,
    defining: frozenset[FaceNode],
    constituent: frozenset[FaceNode],
    run_axis: str,
    width_axis: str,
    open_sign: int,
) -> CylindricalChannelProof | None:
    if len(defining) != 2 or not defining <= constituent:
        return None
    r, w = "xyz".index(run_axis), "xyz".index(width_axis)
    if r == w or open_sign not in {-1, 1}:
        return None
    d = next(i for i in range(3) if i not in (r, w))
    if any(not graph.is_planar(node) for node in defining):
        return None
    walls = {}
    for node in defining:
        normal = graph.normal(node)
        if normal is None or abs(normal[w]) < 1 - 1e-8:
            return None
        sign = 1 if normal[w] > 0 else -1
        if sign in walls:
            return None
        walls[sign] = node
    if set(walls) != {-1, 1}:
        return None
    # Legacy rounded bounds can include an exterior coplanar-facing stock face.
    # Evidence is a candidate hint, not authority to publish that extra face.
    floors = tuple(
        node
        for node in constituent - defining
        if graph.is_planar(node)
        and (normal := graph.normal(node)) is not None
        and normal[d] * open_sign >= 1 - 1e-8
        and all(graph.arc(node, wall) == "concave" for wall in defining)
    )
    if len(floors) != 1:
        return None
    floor = floors[0]
    supports = tuple(sorted((*defining, floor), key=lambda n: n.index))
    owner = graph.common_valid_solid(supports)
    if owner is None:
        return None
    wall_bounds = {sign: _bounds(graph.face(node)) for sign, node in walls.items()}
    bounds = [
        (
            max(wall_bounds[1][i][0], wall_bounds[-1][i][0]),
            min(wall_bounds[1][i][1], wall_bounds[-1][i][1]),
        )
        for i in range(3)
    ]
    bounds[w] = (sum(wall_bounds[1][w]) / 2, sum(wall_bounds[-1][w]) / 2)
    if any(hi - lo <= 1e-6 for lo, hi in bounds):
        return None
    floor_at, mouth = bounds[d][0 if open_sign == 1 else 1], bounds[d][1 if open_sign == 1 else 0]
    if not _at(graph.face(floor), d, floor_at) or any(
        not _at(graph.face(walls[sign]), w, bounds[w][0 if sign == 1 else 1]) for sign in walls
    ):
        return None
    contexts = set.intersection(*(set(graph.neighbours(node)) for node in supports)) - set(supports)
    contexts = {n for n in contexts if all(graph.arc(node, n) == "convex" for node in supports)}
    for cylinder_node in sorted(contexts, key=lambda n: n.index):
        fact = surfaces.fact(cylinder_node)
        if (
            not isinstance(fact, AnalyticSurfaceFact)
            or fact.kind != SurfaceKind.CYLINDER
            or fact.provenance != SurfaceProvenance.NATIVE
        ):
            continue
        centre, axis, radius = (
            Vector(*fact.parameters[:3]),
            Vector(*fact.parameters[3:6]),
            fact.parameters[6],
        )
        run = Vector(*(float(i == r) for i in range(3)))
        if abs(axis.dot(run)) > 1e-8:
            continue
        # A zero-area corner tangent can survive the cap/area checks below.
        # The observed end must remain a single-valued cylinder branch strictly
        # inside its domain over the entire (convex rectangular) footprint.
        transverse = axis.cross(run).normalized()
        max_offset = max(abs((Vector(*p) - centre).dot(transverse)) for p in product(*bounds))
        if radius - max_offset <= 1e-6:
            continue
        source_cylinder = graph.face(cylinder_node)
        point = source_cylinder.position_at(0.37, 0.41)
        delta = point - centre
        radial = delta - axis * delta.dot(axis)
        if source_cylinder.normal_at(point).dot(radial.normalized()) > -1 + 1e-6:
            continue
        for planar in sorted(contexts - {cylinder_node}, key=lambda n: n.index):
            if not graph.is_planar(planar):
                continue
            normal = graph.normal(planar)
            if normal is None or abs(normal[r]) < 1 - 1e-8:
                continue
            end = 1 if normal[r] < 0 else 0
            end_sign = 1 if end == 1 else -1
            far = sum(_bounds(graph.face(planar))[r]) / 2
            if not _at(graph.face(planar), r, far) or (tuple(centre)[r] - far) * end_sign <= 1e-6:
                continue
            if graph.common_valid_solid((*supports, cylinder_node, planar)) != owner:
                continue
            half_bounds = list(bounds)
            half_bounds[r] = tuple(sorted((far, tuple(centre)[r])))
            dx, dy, dz = (hi - lo for lo, hi in half_bounds)
            half = Solid.make_box(
                dx, dy, dz, plane=Plane(origin=tuple(lo for lo, _ in half_bounds))
            )
            along = [(Vector(*p) - centre).dot(axis) for p in product(*half_bounds)]
            margin = max(1.0, radius) * 1e-4
            cylinder = Solid.make_cylinder(
                radius,
                max(along) - min(along) + 2 * margin,
                Plane(origin=centre + axis * (min(along) - margin), z_dir=axis),
            )
            clipped = _shape(half.cut(cylinder))
            if len(clipped.solids()) != 1 or clipped.volume <= 1e-12 or not clipped.is_valid:
                continue
            planes = tuple(f for f in clipped.faces() if f.geom_type == GeomType.PLANE)
            curves = tuple(f for f in clipped.faces() if f.geom_type == GeomType.CYLINDER)
            if not curves:
                continue
            surface_min = min(_bounds(f)[r][0] for f in curves)
            surface_max = max(_bounds(f)[r][1] for f in curves)
            if (surface_min - far if end == 1 else far - surface_max) <= 1e-6:
                continue
            # No artificial end cap on the cylinder's axis plane may survive.
            if any(
                not (
                    _at(f, r, far)
                    or _at(f, d, mouth)
                    or _at(f, d, floor_at)
                    or _at(f, w, bounds[w][0])
                    or _at(f, w, bounds[w][1])
                )
                for f in planes
            ):
                continue
            far_patches = tuple(f for f in planes if _at(f, r, far))
            expected_far = _rectangle(cast(Bounds3, tuple(half_bounds)), r, far)
            if not covered_patch(expected_far, far_patches):
                continue
            side_patches = tuple(f for f in planes if not _at(f, r, far) and not _at(f, d, mouth))
            source_supports = tuple(graph.face(node) for node in supports)
            if any(not covered_patch(f, source_supports) for f in side_patches) or any(
                not covered_patch(f, side_patches) for f in source_supports
            ):
                continue
            solid = graph.solid_shape(owner)
            if material_fraction(solid, clipped) > 1e-9:
                continue
            thickness = max(2e-5, radius * 1e-4)
            lateral = Vector(*(float(i == d) * open_sign for i in range(3)))
            openings = tuple(
                _shape(clipped.translate(direction * thickness).cut(clipped))
                for direction in (-run, run, lateral)
            )
            if any(p.volume <= 1e-12 or material_fraction(solid, p) > 1e-9 for p in openings):
                continue
            centroid = [(lo + hi) / 2 for lo, hi in bounds]
            centroid[r] = 0.0
            delta = Vector(*centroid) - centre
            a = 1 - axis.dot(run) ** 2
            b = 2 * (delta.dot(run) - delta.dot(axis) * axis.dot(run))
            c = delta.dot(delta) - delta.dot(axis) ** 2 - radius**2
            discriminant = b * b - 4 * a * c
            if discriminant <= 0:
                continue
            height = (-b + (-1 if end == 1 else 1) * math.sqrt(discriminant)) / (2 * a)
            interval = (far, height) if end == 1 else (height, far)
            return CylindricalChannelProof(
                supports,
                cylinder_node,
                planar,
                owner,
                run_axis,
                width_axis,
                open_sign,
                cast(Bounds3, tuple(bounds)),
                interval,
                end,
                cast(Vector3, tuple(centre)),
                cast(Vector3, tuple(axis)),
                radius,
                clipped.volume,
            )
    return None


def prove_cylindrical_channel(
    graph: FaceGraph,
    surfaces: EffectiveSurfaceQuery,
    defining: frozenset[FaceNode],
    constituent: frozenset[FaceNode],
    *,
    run_axis: str,
    width_axis: str,
    open_sign: int,
) -> CylindricalChannelProof | None:
    """Fail closed without inventing support, end caps or cross-body authority."""
    try:
        return _prove(graph, surfaces, defining, constituent, run_axis, width_axis, open_sign)
    except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return None
