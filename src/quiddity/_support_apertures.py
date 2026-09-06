# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Private ADR-0021 proof of finite native circular apertures inside support patches."""

from __future__ import annotations

import math
from dataclasses import dataclass

from build123d import Face, GeomType, Plane, Solid, Vector, Wire

from quiddity._adjacency import FaceGraph, FaceNode
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact,
    EffectiveSurfaceQuery,
    SurfaceKind,
    SurfaceProvenance,
)
from quiddity._support_patches import covered_patch
from quiddity._volume_probe import material_fraction


@dataclass(frozen=True, slots=True)
class SupportAperture:
    disk: Face  # Private missing-support explanation, never original evidence.
    cylinder: FaceNode


def _aperture(
    graph: FaceGraph,
    surfaces: EffectiveSurfaceQuery,
    support: FaceNode,
    patch: Face,
    wire: Wire,
) -> SupportAperture | None:
    face = graph.face(support)
    normal_value = graph.normal(support)
    if normal_value is None:
        return None
    normal = Vector(*normal_value)
    edges = tuple(wire.edges())
    if len(edges) != 1 or edges[0].geom_type != GeomType.CIRCLE:
        return None
    disk = Face(wire)
    if (
        disk.distance_to(face.outer_wire()) <= 1e-6
        or disk.distance_to(patch.outer_wire()) <= 1e-6
        or not covered_patch(disk, (patch,))
    ):
        return None
    for neighbour in graph.neighbours(support):
        if not any(edges[0].is_same(edge) for edge in graph.shared_edges(support, neighbour)):
            continue
        fact = surfaces.fact(neighbour)
        if (
            not isinstance(fact, AnalyticSurfaceFact)
            or fact.kind != SurfaceKind.CYLINDER
            or fact.provenance != SurfaceProvenance.NATIVE
        ):
            continue
        centre = Vector(*fact.parameters[:3])
        axis = Vector(*fact.parameters[3:6])
        radius = fact.parameters[6]
        if abs(abs(normal.dot(axis)) - 1) > 1e-8:
            continue
        source = graph.face(neighbour)
        point = source.position_at(0.37, 0.41)
        delta = point - centre
        radial = delta - axis * delta.dot(axis)
        if source.normal_at(point).dot(radial.normalized()) > -1 + 1e-6:
            continue
        rings = [edge for edge in source.edges() if edge.geom_type == GeomType.CIRCLE]
        if len(rings) != 2 or any(abs(edge.length - 2 * math.pi * radius) > 1e-6 for edge in rings):
            continue
        contexts: list[FaceNode] = []
        for ring in rings:
            neighbours = [
                node
                for node in graph.neighbours(neighbour)
                if graph.is_planar(node)
                and any(ring.is_same(edge) for edge in graph.shared_edges(neighbour, node))
            ]
            if len(neighbours) != 1:
                break
            contexts.extend(neighbours)
        if len(contexts) != 2:
            continue
        owner = graph.common_valid_solid((support, neighbour, *contexts))
        if owner is None:
            continue
        ends = sorted((ring.arc_center - centre).dot(axis) for ring in rings)
        if ends[1] - ends[0] <= 1e-6:
            continue
        cell = Solid.make_cylinder(
            radius,
            ends[1] - ends[0],
            Plane(origin=centre + axis * ends[0], z_dir=axis),
        )
        sides = tuple(p for p in cell.faces() if p.geom_type == GeomType.CYLINDER)
        if not covered_patch(source, sides) or any(
            not covered_patch(side, (source,)) for side in sides
        ):
            continue
        if material_fraction(graph.solid_shape(owner), cell) > 1e-9:
            continue
        if abs(disk.area - math.pi * radius**2) > 1e-6:
            continue
        return SupportAperture(disk, neighbour)
    return None


def proved_support_apertures(
    graph: FaceGraph, surfaces: EffectiveSurfaceQuery, support: FaceNode, patch: Face
) -> tuple[SupportAperture, ...]:
    """Return only complete, strictly interior, same-owner aperture explanations."""
    if not graph.is_planar(support):
        return ()
    results = []
    for wire in graph.face(support).inner_wires():
        try:
            proof = _aperture(graph, surfaces, support, patch, wire)
        except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
            continue
        if proof is not None:
            results.append(proof)
    return tuple(results)
