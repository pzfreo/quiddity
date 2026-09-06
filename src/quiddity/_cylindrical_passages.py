# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Original wall-cycle proof for a polygonal passage meeting a native cross-bore."""

from __future__ import annotations

import math
from dataclasses import dataclass
from itertools import combinations, product
from typing import cast

from build123d import Compound, Face, GeomType, Plane, Shape, ShapeList, Solid, Vector, Wire

from quiddity._adjacency import FaceGraph, FaceNode, SolidRef, connected_components
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact,
    EffectiveSurfaceQuery,
    SurfaceKind,
    SurfaceProvenance,
)
from quiddity._section_passages import _ordered_cycle, _pair_line
from quiddity._sections import LocalFrame, PlanarSection, SectionVertex
from quiddity._support_patches import covered_patch
from quiddity._volume_probe import material_fraction

Vector3 = tuple[float, float, float]


@dataclass(frozen=True, slots=True)
class CylindricalPassageProof:
    walls: tuple[FaceNode, ...]
    cylinder: FaceNode
    planar_context: FaceNode
    owner: SolidRef
    frame: LocalFrame
    section: PlanarSection
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


def _cell_proof(
    graph: FaceGraph,
    planar: FaceNode,
    cylinder: FaceNode,
    fact: AnalyticSurfaceFact,
    walls: tuple[FaceNode, ...],
    base: LocalFrame,
) -> CylindricalPassageProof | None:
    owner = graph.common_valid_solid((*walls, planar, cylinder))
    if owner is None or len(walls) < 3:
        return None
    adjacency = {n: {m for m in walls if m != n and graph.arc(n, m) == "concave"} for n in walls}
    if any(len(neighbours) != 2 for neighbours in adjacency.values()):
        return None
    # Caller supplies a connected component, so degree two now means one cycle.
    lines = {}
    for left, right in combinations(walls, 2):
        if right in adjacency[left]:
            line = _pair_line(graph, left, right, base)
            if line is None:
                return None
            lines[frozenset((left, right))] = line
    order = _ordered_cycle(walls, adjacency, lines)
    if len(set(order)) != len(walls):
        return None
    points = tuple(
        lines[frozenset((node, order[(i + 1) % len(order)]))][:2] for i, node in enumerate(order)
    )
    raw = PlanarSection(tuple(SectionVertex(point) for point in points))
    points = tuple(vertex.point for vertex in raw.boundary)
    turns = []
    for i, p in enumerate(points):
        before, after = points[i - 1], points[(i + 1) % len(points)]
        turns.append(
            (p[0] - before[0]) * (after[1] - p[1]) - (p[1] - before[1]) * (after[0] - p[0])
        )
    if min(turns) < 0:
        return None
    run = Vector(*base.run)
    centre, axis, radius = (
        Vector(*fact.parameters[:3]),
        Vector(*fact.parameters[3:6]),
        fact.parameters[6],
    )
    normal = Vector(*cast(Vector3, graph.normal(planar)))
    far = graph.face(planar).center().dot(run)
    end = 1 if normal.dot(run) < 0 else 0
    sign = 1 if end == 1 else -1
    if (centre.dot(run) - far) * sign <= 1e-6:
        return None

    def world(point: tuple[float, float], height: float) -> Vector:
        return Vector(*base.u) * point[0] + Vector(*base.v) * point[1] + run * height

    transverse = axis.cross(run).normalized()
    offsets = tuple((world(point, far) - centre).dot(transverse) for point in points)
    if radius - max(abs(q) for q in offsets) <= 1e-6:
        return None
    # Bound the entire branch, including an interior crest, with the source
    # axis tilt retained. q and the centre-line correction are affine over the
    # convex domain. Their separate extrema give a conservative separation proof.
    qmin = 0.0 if min(offsets) <= 0 <= max(offsets) else min(abs(q) for q in offsets)
    k = axis.dot(run)
    a = 1 - k * k
    deltas = tuple(world(point, 0) - centre for point in points)
    centres = tuple(-(delta.dot(run) - delta.dot(axis) * k) / a for delta in deltas)
    radius_height = math.sqrt((radius * radius - qmin * qmin) / a)
    separation = (
        min(centres) - radius_height - far if end == 1 else far - max(centres) - radius_height
    )
    if separation <= 1e-6:
        return None
    mouth = Face(Wire.make_polygon([world(point, far) for point in points], close=True))
    half = Solid.extrude(mouth, run * (centre.dot(run) - far))
    axial = tuple(
        (world(point, t) - centre).dot(axis) for point, t in product(points, (far, centre.dot(run)))
    )
    margin = max(1.0, radius) * 1e-4
    bore = Solid.make_cylinder(
        radius,
        max(axial) - min(axial) + 2 * margin,
        Plane(origin=centre + axis * (min(axial) - margin), z_dir=axis),
    )
    cell = _shape(half.cut(bore))
    if not cell.is_valid or len(cell.solids()) != 1 or cell.volume <= 1e-12:
        return None
    planes = tuple(f for f in cell.faces() if f.geom_type == GeomType.PLANE)
    curves = tuple(f for f in cell.faces() if f.geom_type == GeomType.CYLINDER)
    if not curves or len(planes) + len(curves) != len(cell.faces()):
        return None
    sides = tuple(f for f in planes if abs(f.normal_at().dot(run)) < 1e-8)
    end_faces = tuple(f for f in planes if f not in sides)
    if any(
        abs(abs(f.normal_at().dot(run)) - 1) > 1e-8
        or any(abs(Vector(*v).dot(run) - far) > 1e-6 for v in f.vertices())
        for f in end_faces
    ):
        return None
    if not covered_patch(mouth, end_faces) or any(
        not covered_patch(f, (mouth,)) for f in end_faces
    ):
        return None
    sources = tuple(graph.face(n) for n in walls)
    if any(not covered_patch(f, sources) for f in sides) or any(
        not covered_patch(f, sides) for f in sources
    ):
        return None
    solid = graph.solid_shape(owner)
    if material_fraction(solid, cell) > 1e-9:
        return None
    for direction in (-run, run):
        opening = _shape(cell.translate(direction * max(2e-5, radius * 1e-4)).cut(cell))
        if opening.volume <= 1e-12 or material_fraction(solid, opening) > 1e-9:
            return None
    centroid = raw.centroid
    origin = world(centroid, 0)
    frame = LocalFrame.canonical(base.run, cast(Vector3, tuple(origin)))
    section = PlanarSection(
        tuple(SectionVertex((p[0] - centroid[0], p[1] - centroid[1])) for p in points)
    )
    delta = origin - centre
    b = 2 * (delta.dot(run) - delta.dot(axis) * k)
    c = delta.dot(delta) - delta.dot(axis) ** 2 - radius * radius
    discriminant = b * b - 4 * a * c
    if discriminant <= 0:
        return None
    height = (-b - sign * math.sqrt(discriminant)) / (2 * a)
    interval = (far, height) if end == 1 else (height, far)
    return CylindricalPassageProof(
        tuple(sorted(walls, key=lambda n: n.index)),
        cylinder,
        planar,
        owner,
        frame,
        section,
        interval,
        end,
        cast(Vector3, tuple(centre)),
        cast(Vector3, tuple(axis)),
        radius,
        cell.volume,
    )


def _native_bores(
    graph: FaceGraph, surfaces: EffectiveSurfaceQuery
) -> tuple[tuple[FaceNode, AnalyticSurfaceFact], ...]:
    bores = []
    for node in graph.nodes:
        try:
            fact = surfaces.fact(node)
            if (
                not isinstance(fact, AnalyticSurfaceFact)
                or fact.kind != SurfaceKind.CYLINDER
                or fact.provenance != SurfaceProvenance.NATIVE
            ):
                continue
            axis, centre = Vector(*fact.parameters[3:6]), Vector(*fact.parameters[:3])
            source = graph.face(node)
            point = source.position_at(0.37, 0.41)
            delta = point - centre
            radial = delta - axis * delta.dot(axis)
            if source.normal_at(point).dot(radial.normalized()) <= -1 + 1e-6:
                bores.append((node, fact))
        except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
            continue
    return tuple(bores)


def cylindrical_passage_proofs(
    graph: FaceGraph, surfaces: EffectiveSurfaceQuery
) -> tuple[CylindricalPassageProof, ...]:
    """Discover original closed wall cycles; expected side counts are not inputs."""
    results = []
    bores = _native_bores(graph, surfaces)
    for planar in graph.nodes:
        if not graph.is_planar(planar) or (normal := graph.normal(planar)) is None:
            continue
        base = LocalFrame.canonical(normal, (0, 0, 0))
        run = Vector(*base.run)
        for cylinder, fact in bores:
            axis = Vector(*fact.parameters[3:6])
            if abs(axis.dot(run)) > 1e-8:
                continue
            candidates = tuple(
                n
                for n in set(graph.neighbours(planar)) & set(graph.neighbours(cylinder))
                if graph.is_planar(n)
                and (wall_normal := graph.normal(n)) is not None
                and abs(Vector(*wall_normal).dot(run)) < 1e-8
                and graph.arc(planar, n) == "convex"
                and graph.arc(cylinder, n) == "convex"
            )
            for walls in connected_components(
                candidates, lambda a, b: graph.arc(a, b) == "concave"
            ):
                try:
                    proof = _cell_proof(graph, planar, cylinder, fact, walls, base)
                except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
                    continue
                if proof is not None:
                    results.append(proof)
    return tuple(results)
