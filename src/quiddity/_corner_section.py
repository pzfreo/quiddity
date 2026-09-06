# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Exact source-face proof for an accepted principal-axis corner notch.

The old Pocket extents do not establish a profile. This adapter independently checks its three
source faces and returns only the two physical wall segments, never an invented box closure.
"""

from __future__ import annotations

from dataclasses import dataclass
from itertools import combinations, product

from OCP.BRepAdaptor import BRepAdaptor_Curve
from OCP.GeomAbs import GeomAbs_Line

from quiddity._adjacency import FaceGraph, FaceNode
from quiddity._section_passages import _end_slab, _probe_prism
from quiddity._sections import LocalFrame, PlanarSection, SectionVertex
from quiddity._volume_probe import material_fraction as _material_fraction


@dataclass(frozen=True)
class CornerSectionProof:
    run_interval: tuple[float, float]
    open_sign: int
    boundary: tuple[tuple[float, float], ...]


def prove_corner_section(
    graph: FaceGraph,
    nodes: frozenset[FaceNode],
    axis: str,
) -> CornerSectionProof | None:
    """Prove complete orthogonal rectangular walls and floor from exact same-body topology."""

    if axis not in "xyz" or len(axis) != 1 or len(nodes) != 3:
        return None
    owner = graph.common_valid_solid(nodes)
    if owner is None:
        return None
    if any(second not in graph.neighbours(first) for first, second in combinations(nodes, 2)):
        return None
    # One rectangular planar face per principal normal, all bounded by physical straight edges.
    rectangles = {}
    normals = {}
    by_axis = {}
    for node in nodes:
        normal = graph.normal(node) if graph.is_planar(node) else None
        if normal is None:
            return None
        direction = normal
        fixed = max(range(3), key=lambda index: abs(direction[index]))
        if abs(normal[fixed]) < 1 - 1e-8 or fixed in rectangles:
            return None
        face = graph.face(node)
        edges = graph.edges(node)
        points = [tuple(vertex) for vertex in face.vertices()]
        if (
            len(edges) != 4
            or len(points) != 4
            or any(BRepAdaptor_Curve(edge.wrapped).GetType() != GeomAbs_Line for edge in edges)
        ):
            return None
        bounds = tuple((min(p[i] for p in points), max(p[i] for p in points)) for i in range(3))
        varying = [i for i in range(3) if i != fixed]
        tolerance = max(1e-7, max(high - low for low, high in bounds) * 1e-7)
        if bounds[fixed][1] - bounds[fixed][0] > tolerance:
            return None
        if any(bounds[i][1] - bounds[i][0] <= tolerance for i in varying):
            return None
        # Four lines alone can bound a trapezoid. Require each actual corner of the rectangle.
        if any(
            not any(
                all(abs(point[i] - target[j]) <= tolerance for j, i in enumerate(varying))
                for point in points
            )
            for target in product(*(bounds[i] for i in varying))
        ):
            return None
        area = (bounds[varying[0]][1] - bounds[varying[0]][0]) * (
            bounds[varying[1]][1] - bounds[varying[1]][0]
        )
        if abs(face.area - area) > area * 1e-6:
            return None
        rectangles[fixed] = bounds
        normals[fixed] = normal[fixed]
        by_axis[fixed] = node

    run = "xyz".index(axis)
    u, v = [i for i in range(3) if i != run]
    floor, first, second = rectangles[run], rectangles[u], rectangles[v]
    tolerance = max(
        1e-7, max(hi - lo for bounds in rectangles.values() for lo, hi in bounds) * 1e-7
    )

    def same_span(left, right):
        return all(abs(a - b) <= tolerance for a, b in zip(left, right, strict=True))

    if not (
        same_span(first[run], second[run])
        and same_span(first[v], floor[v])
        and same_span(second[u], floor[u])
    ):
        return None
    low, high = first[run]
    sign = 1 if normals[run] > 0 else -1
    if abs(floor[run][0] - (low if sign == 1 else high)) > tolerance:
        return None
    mouth = high if sign == 1 else low
    for wall_axis in (u, v):
        if not any(
            graph.is_planar(neighbour)
            and (normal := graph.normal(neighbour)) is not None
            and normal[run] * sign > 1 - 1e-8
            and all(
                abs(tuple(vertex)[run] - mouth) <= tolerance
                for vertex in graph.face(neighbour).vertices()
            )
            for neighbour in graph.neighbours(by_axis[wall_axis])
        ):
            return None  # no planar exterior mouth: a cap or treatment is not proved open
    # Each wall's outward normal must point INTO the removed quadrant. This rejects a boss
    # corner even though its three rectangular faces have the same incidence and extents.
    first_at, second_at = first[u][0], second[v][0]
    if abs(first_at - floor[u][0 if normals[u] > 0 else 1]) > tolerance:
        return None
    if abs(second_at - floor[v][0 if normals[v] > 0 else 1]) > tolerance:
        return None
    far_u = floor[u][1 if normals[u] > 0 else 0]
    far_v = floor[v][1 if normals[v] > 0 else 0]
    # The three faces alone do not exclude a suspended same-body obstruction. Sweep the
    # independently proved *whole floor rectangle*, not a diagonal closure of the L chain.
    # This rectangle is an internal material probe, never published as an observed wall.
    try:
        centre = tuple((lo + hi) / 2 for lo, hi in floor)
        frame = LocalFrame.principal(axis, (centre[0], centre[1], centre[2]))
        local_axes = (v, u) if axis == "y" else (u, v)
        half_u, half_v = ((floor[i][1] - floor[i][0]) / 2 for i in local_axes)
        section = PlanarSection(
            tuple(
                SectionVertex(point)
                for point in (
                    (-half_u, -half_v),
                    (half_u, -half_v),
                    (half_u, half_v),
                    (-half_u, half_v),
                )
            )
        )
        solid = graph.solid_shape(owner)
        thickness = max(2e-5, max(1.0, high - low) * 1e-4, (half_u**2 + half_v**2) ** 0.5 * 1e-4)
        if (
            _material_fraction(solid, _probe_prism(frame, (low, high), section)) > 1e-9
            or _material_fraction(solid, _end_slab(frame, mouth, sign, thickness, section)) > 1e-9
        ):
            return None
    except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return None
    return CornerSectionProof(
        (round(low, 3), round(high, 3)),
        sign,
        ((first_at, far_v), (first_at, second_at), (far_u, second_at)),
    )
