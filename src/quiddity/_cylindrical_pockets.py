# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Private exact native-cylinder termination proofs; no public record construction."""

from __future__ import annotations

from dataclasses import dataclass
from typing import cast

from build123d import Compound, GeomType, Plane, Shape, ShapeList, Solid, Vector

from quiddity._adjacency import FaceGraph, FaceNode, SolidRef
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact,
    EffectiveSurfaceQuery,
    SurfaceKind,
    SurfaceProvenance,
)
from quiddity._support_patches import covered_patch
from quiddity._volume_probe import material_fraction


@dataclass(frozen=True, slots=True)
class CylindricalPocketProof:
    floor: FaceNode
    walls: tuple[FaceNode, ...]
    stock: FaceNode
    owner: SolidRef
    axis_point: tuple[float, float, float]
    axis_direction: tuple[float, float, float]
    run: tuple[float, float, float]
    radius: float
    volume: float


def _shape(value: Shape | ShapeList | None) -> Shape:
    if value is None:
        return Compound([])
    return Compound(value) if isinstance(value, ShapeList) else value


def _proofs(
    graph: FaceGraph, surfaces: EffectiveSurfaceQuery, floor: FaceNode
) -> tuple[CylindricalPocketProof, ...]:
    results: list[CylindricalPocketProof] = []
    if not graph.is_planar(floor):
        return ()
    source = graph.face(floor)
    if len(source.wires()) != 1:
        return ()
    normal = graph.normal(floor)
    if normal is None:
        return ()
    run = Vector(*normal)
    walls = tuple(n for n in graph.neighbours(floor) if graph.arc(floor, n) == "concave")
    if len(walls) < 3 or any(
        not graph.is_planar(n)
        or graph.normal(n) is None
        or abs(Vector(*cast(tuple[float, float, float], graph.normal(n))).dot(run)) > 1e-8
        for n in walls
    ):
        return ()
    # A closed polygonal footprint needs a real wall along EVERY floor edge.
    # Area equality alone misses a sideways breakout with a zero-area fourth wall.
    floor_edges = tuple(source.edges())
    shared = tuple(edge for wall in walls for edge in graph.shared_edges(floor, wall))
    if any(
        edge.geom_type != GeomType.LINE or not any(edge.is_same(other) for other in shared)
        for edge in floor_edges
    ):
        return ()
    contexts = {n for wall in walls for n in graph.neighbours(wall)} - set(walls) - {floor}
    for stock in sorted(contexts, key=lambda n: n.index):
        fact = surfaces.fact(stock)
        if not isinstance(fact, AnalyticSurfaceFact) or fact.kind != SurfaceKind.CYLINDER:
            continue
        if fact.provenance != SurfaceProvenance.NATIVE:
            continue
        if not all(graph.arc(wall, stock) == "convex" for wall in walls):
            continue
        owner = graph.common_valid_solid((floor, *walls, stock))
        if owner is None:
            continue
        centre, axis, radius = (
            Vector(*fact.parameters[:3]),
            Vector(*fact.parameters[3:6]),
            fact.parameters[6],
        )
        if abs(axis.dot(run)) > 1e-8:
            continue
        # Initial bounded branch: floor lies on the outward side of the cylinder axis.
        if (source.center() - centre).dot(run) <= 1e-6:
            continue
        # For this planar line-bounded floor, radial offset is affine on each edge;
        # its maximum is at a vertex. This proves strictly positive separation on
        # the complete polygon, including boundaries of zero area.
        separated = True
        for vertex in source.vertices():
            delta = Vector(*vertex) - centre
            height = delta.dot(run)
            transverse = delta - axis * delta.dot(axis) - run * height
            discriminant = radius**2 - transverse.length**2
            if discriminant <= 0 or discriminant**0.5 - height <= 1e-6:
                separated = False
                break
        if not separated:
            continue
        centre += axis * (source.center() - centre).dot(axis)
        span = max(radius * 4, source.bounding_box().diagonal * 4)
        cylinder = Solid.make_cylinder(
            radius, span * 2, Plane(origin=centre - axis * span, z_dir=axis)
        )
        sweep = Solid.extrude(source, run * (radius * 4))
        clipped = _shape(sweep.intersect(cylinder))
        if len(clipped.solids()) != 1 or clipped.volume <= 1e-12:
            continue
        floor_patches = tuple(
            f
            for f in clipped.faces()
            if f.geom_type == GeomType.PLANE
            and abs(abs(f.normal_at().dot(run)) - 1) < 1e-8
            and abs((f.center() - source.center()).dot(run)) < 1e-6
        )
        if not covered_patch(source, floor_patches):
            continue
        if any(not covered_patch(f, (source,)) for f in floor_patches):
            continue
        side_patches = tuple(
            f
            for f in clipped.faces()
            if f.geom_type == GeomType.PLANE and abs(f.normal_at().dot(run)) < 1e-8
        )
        supports = tuple(graph.face(n) for n in walls)
        if any(not covered_patch(f, supports) for f in side_patches):
            continue
        if any(not covered_patch(f, side_patches) for f in supports):
            continue
        solid = graph.solid_shape(owner)
        if material_fraction(solid, clipped) > 1e-9:
            continue
        thickness = max(2e-5, radius * 1e-4)
        if material_fraction(solid, Solid.extrude(source, -run * thickness)) < 1 - 1e-9:
            continue
        expanded = _shape(sweep.intersect(cylinder.translate(run * thickness)))
        mouth = _shape(expanded.cut(clipped))
        if mouth.volume <= 1e-12 or material_fraction(solid, mouth) > 1e-9:
            continue
        results.append(
            CylindricalPocketProof(
                floor,
                tuple(sorted(walls, key=lambda n: n.index)),
                stock,
                owner,
                cast(tuple[float, float, float], tuple(centre)),
                cast(tuple[float, float, float], tuple(axis)),
                cast(tuple[float, float, float], tuple(run)),
                radius,
                clipped.volume,
            )
        )
    return tuple(results)


def cylindrical_pocket_proofs(
    graph: FaceGraph, surfaces: EffectiveSurfaceQuery
) -> tuple[CylindricalPocketProof, ...]:
    """Fail closed on kernel refusal; return only original-source proof authority."""
    results: list[CylindricalPocketProof] = []
    for floor in graph.nodes:
        try:
            results.extend(_proofs(graph, surfaces, floor))
        except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
            continue
    return tuple(results)
