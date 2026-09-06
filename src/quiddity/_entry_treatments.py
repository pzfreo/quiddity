# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Finite native planar entry-bevel proofs for an otherwise constant wall ring.

Generated treatment-cell faces are internal proof supports, never source evidence.
"""

from __future__ import annotations

import itertools
import math
from dataclasses import dataclass

from build123d import Face, Shell, Solid, Vector, Wire

from quiddity._adjacency import FaceGraph, FaceNode
from quiddity._support_patches import covered_patch
from quiddity._volume_probe import material_fraction

Vector3 = tuple[float, float, float]
PlaneEquation = tuple[Vector, float]


@dataclass(frozen=True, slots=True)
class EntryTreatmentProof:
    treatments: frozenset[FaceNode]
    stock: frozenset[FaceNode]


def _plane(graph: FaceGraph, node: FaceNode, factor: float = 1.0) -> PlaneEquation:
    normal = graph.normal(node)
    if normal is None:
        raise ValueError("treatment cell needs planar source faces")
    direction = Vector(*normal) * factor
    return direction, direction.dot(graph.face(node).center())


def _cell_supports(
    graph: FaceGraph,
    seed: frozenset[FaceNode],
    wall: FaceNode,
    bevel: FaceNode,
    run: Vector3,
    far: float,
    sign: float,
) -> tuple[Face, ...] | None:
    # Inside the removed cell: outside the base wall, below the stock end,
    # above the bevel and inside the other convex polygon support planes.
    planes = [_plane(graph, bevel), _plane(graph, wall, -1), (Vector(*run) * -sign, -sign * far)]
    planes.extend(
        _plane(graph, node) for node in sorted(seed, key=lambda n: n.index) if node != wall
    )
    points: list[Vector] = []
    for triple in itertools.combinations(planes, 3):
        (first, a), (second, b), (third, c) = triple
        determinant = first.dot(second.cross(third))
        if abs(determinant) < 1e-10:
            continue
        point = (
            second.cross(third) * a + third.cross(first) * b + first.cross(second) * c
        ) / determinant
        if all(n.dot(point) >= d - 1e-7 for n, d in planes) and not any(
            (point - other).length < 1e-7 for other in points
        ):
            points.append(point)
    if len(points) < 4:
        return None
    faces: list[Face] = []
    bevel_face = None
    for index, (normal, at) in enumerate(planes):
        vertices = [p for p in points if abs(normal.dot(p) - at) < 1e-7]
        if len(vertices) < 3:
            continue
        centre = sum(vertices, Vector()) / len(vertices)
        u = vertices[0] - centre
        norm = u.length
        if norm <= 1e-12:
            return None
        u = u / norm
        v = (-normal).cross(u)
        vertices.sort(key=lambda p: math.atan2((p - centre).dot(v), (p - centre).dot(u)))
        face = Face(Wire.make_polygon([*vertices, vertices[0]]))
        faces.append(face)
        if index == 0:
            bevel_face = face
    if bevel_face is None:
        return None
    source = graph.face(bevel)
    if not covered_patch(source, (bevel_face,)) or not covered_patch(bevel_face, (source,)):
        return None
    cell = Solid(Shell(faces))
    if not cell.is_valid or cell.volume <= 1e-12:
        return None
    owner = graph.common_valid_solid((*seed, bevel))
    if owner is None or material_fraction(graph.solid_shape(owner), cell) > 1e-9:
        return None
    return tuple(cell.faces())


def prove_entry_treatments(
    graph: FaceGraph,
    seed: frozenset[FaceNode],
    wire: Wire,
    run: Vector3,
    at: float,
    far: float,
) -> EntryTreatmentProof | None:
    """Explain every missing base-wall patch using finite observed planar treatments."""
    sign = 1.0 if far > at else -1.0
    direction = Vector(*run)
    try:
        contexts = {n for wall in seed for n in graph.neighbours(wall)} - seed
        stock = frozenset(
            n
            for n in contexts
            if graph.is_planar(n)
            and (normal := graph.normal(n)) is not None
            and Vector(*normal).dot(direction) * sign > 1 - 1e-8
            and all(abs(Vector(*v).dot(direction) - far) < 1e-6 for v in graph.face(n).vertices())
        )
        if not stock:
            return None
        supports = [graph.face(wall) for wall in seed]
        treatments: set[FaceNode] = set()
        for wall in sorted(seed, key=lambda n: n.index):
            if any(graph.arc(wall, n) in ("convex", "smooth") for n in stock):
                continue
            wall_normal = graph.normal(wall)
            if wall_normal is None:
                return None
            wall_direction = Vector(*wall_normal)
            explained = False
            for bevel in sorted(set(graph.neighbours(wall)) - seed - stock, key=lambda n: n.index):
                if not (graph.is_planar(bevel) and graph.arc(wall, bevel) == "convex"):
                    continue
                bevel_normal = graph.normal(bevel)
                if bevel_normal is None:
                    continue
                bnormal = Vector(*bevel_normal)
                along, across = bnormal.dot(direction), bnormal.dot(wall_direction)
                if along * sign <= 1e-6 or across <= 1e-6:
                    continue
                if (bnormal - direction * along - wall_direction * across).length > 1e-6:
                    continue
                if not any(graph.arc(bevel, n) == "convex" for n in stock):
                    continue
                if set(graph.neighbours(bevel)) - seed - stock:
                    continue
                if graph.common_valid_solid((*seed, *stock, bevel)) is None:
                    continue
                patches = _cell_supports(graph, seed, wall, bevel, run, far, sign)
                if patches is not None:
                    supports.extend(patches)
                    treatments.add(bevel)
                    explained = True
            if not explained:
                return None
        if not treatments:
            return None
        for edge in wire.edges():
            a, b = edge.position_at(0), edge.position_at(1)
            delta = Vector(*run) * (far - at)
            patch = Face(Wire.make_polygon([a, b, b + delta, a + delta, a]))
            if not covered_patch(patch, tuple(supports)):
                return None
        return EntryTreatmentProof(frozenset(treatments), stock)
    except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return None
