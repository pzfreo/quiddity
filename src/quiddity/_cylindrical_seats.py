# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Original-support proof for open, at-most-semicircular cylindrical channels."""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import cast

from build123d import Edge, Face, Solid, Vector, Vertex, Wire
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Cylinder

from quiddity._adjacency import FaceGraph, FaceNode, SolidRef, frame_points_outward
from quiddity._geometry import length_tol
from quiddity._sections import LocalFrame, SectionVertex
from quiddity._support_patches import covered_patch
from quiddity._typing import Vector3
from quiddity._volume_probe import material_fraction


@dataclass(frozen=True, slots=True)
class CylindricalSeatProof:
    walls: tuple[FaceNode, ...]
    context: tuple[FaceNode, ...]
    owner: SolidRef
    frame: LocalFrame
    boundary: tuple[SectionVertex, SectionVertex]
    run_interval: tuple[float, float]


@dataclass(frozen=True, slots=True)
class _Cylinder:
    radius: float
    axis: Vector
    origin: Vector

    def same_surface(self, other: _Cylinder) -> bool:
        tolerance = length_tol(self.radius, rel=1e-6)
        delta = other.origin - self.origin
        return (
            abs(self.radius - other.radius) <= tolerance
            and self.axis.cross(other.axis).length <= 1e-8
            and (delta - self.axis * delta.dot(self.axis)).length <= tolerance
        )


def _prove(
    graph: FaceGraph, walls: tuple[FaceNode, ...], cylinder: _Cylinder
) -> CylindricalSeatProof | None:
    own = frozenset(walls)
    context = tuple(
        sorted({n for w in walls for n in graph.neighbours(w)} - own, key=lambda n: n.index)
    )
    owner = graph.common_valid_solid((*walls, *context))
    if owner is None or not context or any(not graph.is_planar(n) for n in context):
        return None
    radius, axis = cylinder.radius, cylinder.axis
    tolerance = length_tol(radius, rel=1e-6)
    points = [Vector(*tuple(v)) for wall in walls for v in graph.face(wall).vertices()]
    if not points:
        return None
    low, high = min(p.dot(axis) for p in points), max(p.dot(axis) for p in points)
    if high - low <= 2 * tolerance:
        return None
    rims: list[set[Edge]] = [set(), set()]
    lips: set[Edge] = set()
    opening_normal = None
    for wall in walls:
        for node in graph.neighbours(wall):
            if node in own:
                continue
            # Tangent fillets and interior corner blends are not open seats.
            convex_boundary = graph.arc(wall, node) == "convex"
            if not convex_boundary:
                return None
            normal_value = graph.normal(node)
            if normal_value is None:
                return None
            normal = Vector(*normal_value)
            for occurrence in graph.shared_occurrences(wall, node):
                edge = occurrence.edge
                samples = (edge.position_at(0), edge.position_at(0.5), edge.position_at(1))
                if edge.geom_type.name == "CIRCLE":
                    end_index = (
                        0
                        if abs(samples[1].dot(axis) - low) < abs(samples[1].dot(axis) - high)
                        else 1
                    )
                    at, sign = (low, -1) if end_index == 0 else (high, 1)
                    if normal.dot(axis) * sign < 1 - 1e-8 or any(
                        abs(p.dot(axis) - at) > tolerance for p in samples
                    ):
                        return None
                    rims[end_index].add(edge)
                elif edge.geom_type.name == "LINE":
                    delta = samples[-1] - samples[0]
                    if (
                        delta.length <= tolerance
                        or delta.cross(axis).length > tolerance
                        or abs(normal.dot(axis)) > 1e-8
                    ):
                        return None
                    if opening_normal is not None and normal.dot(opening_normal) < 1 - 1e-8:
                        return None
                    opening_normal = normal
                    lips.add(edge)
                else:
                    return None
    if not all(rims) or not lips or opening_normal is None:
        return None
    # A rim may be subdivided at a native seam. Its two degree-one vertices
    # determine the physical arc; all subdivisions retain their source faces.
    counts: dict[Vertex, int] = {}
    for edge in rims[0]:
        for vertex in edge.vertices():
            counts[vertex] = counts.get(vertex, 0) + 1
    ends = [Vector(*tuple(v)) for v, count in counts.items() if count == 1]
    if len(ends) != 2 or any(count not in (1, 2) for count in counts.values()):
        return None
    sweep = sum(edge.length for edge in rims[0]) / radius
    if not 1e-6 < sweep <= math.pi + 1e-8:
        return None
    start, end = ends
    if abs((end - start).dot(opening_normal)) > tolerance:
        return None
    mouth_at = start.dot(opening_normal)
    if any(abs(edge.position_at(0.5).dot(opening_normal) - mouth_at) > tolerance for edge in lips):
        return None
    # The circle's low point opposite the exterior normal fixes which of the
    # two arcs is the concave trough, including exactly semicircular seats.
    centre = cylinder.origin + axis * (low - cylinder.origin.dot(axis))
    middle = centre - opening_normal * radius
    arc = Edge.make_three_point_arc(start, middle, end)
    if abs(arc.length - radius * sweep) > tolerance:
        return None
    section_face = Face(Wire([arc, Edge.make_line(end, start)]))
    prism = Solid.extrude(section_face, axis * (high - low))
    support = tuple(f for f in prism.faces() if f.geom_type.name == "CYLINDER")
    originals = tuple(graph.face(w) for w in walls)
    if (
        len(support) != 1
        or not covered_patch(support[0], originals)
        or any(not covered_patch(f, support) for f in originals)
    ):
        return None
    solid = graph.solid_shape(owner)
    thickness = length_tol(radius, rel=1e-4, floor=2e-5)
    mouth = Face(
        Wire.make_polygon(
            [start, end, end + axis * (high - low), start + axis * (high - low)], close=True
        )
    )
    probes = (
        prism,
        Solid.extrude(section_face.translate(-axis * thickness), axis * (thickness - 1e-6)),
        Solid.extrude(
            section_face.translate(axis * (high - low + 1e-6)), axis * (thickness - 1e-6)
        ),
        Solid.extrude(mouth.translate(opening_normal * 1e-6), opening_normal * (thickness - 1e-6)),
    )
    if any(material_fraction(solid, probe, properties=graph) > 1e-9 for probe in probes):
        return None
    frame = LocalFrame.canonical(
        cast(Vector3, tuple(axis)), cast(Vector3, tuple(section_face.center()))
    )

    def project(point: Vector) -> tuple[float, float]:
        delta = point - Vector(*frame.origin)
        return (delta.dot(Vector(*frame.u)), delta.dot(Vector(*frame.v)))

    first, last = project(start), project(end)
    radial_start, radial_end = start - centre, end - centre
    sign = 1 if radial_start.cross(radial_end).dot(axis) > 0 else -1
    # At a semicircle the cross product vanishes. The midpoint fixes orientation.
    if abs(radial_start.cross(radial_end).dot(axis)) < radius * radius * 1e-8:
        sign = 1 if radial_start.cross(middle - centre).dot(axis) > 0 else -1
    boundary = (SectionVertex(first, sign * math.tan(sweep / 4)), SectionVertex(last))
    return CylindricalSeatProof(walls, context, owner, frame, boundary, (low, high))


def cylindrical_seat_proofs(graph: FaceGraph) -> tuple[CylindricalSeatProof, ...]:
    cylinders = {}
    for node in graph.nodes:
        if (
            graph.surface(node) != GeomAbs_Cylinder
            or frame_points_outward(graph.face(node)) is not False
        ):
            continue
        cylinder = BRepAdaptor_Surface(graph.face(node).wrapped).Cylinder()
        frame = LocalFrame.canonical(
            cast(Vector3, cylinder.Axis().Direction().Coord()), (0.0, 0.0, 0.0)
        )
        cylinders[node] = _Cylinder(
            cylinder.Radius(), Vector(*frame.run), Vector(*cylinder.Axis().Location().Coord())
        )
    remaining = set(cylinders)
    proofs = []
    for seed in cylinders:
        if seed not in remaining:
            continue
        remaining.remove(seed)
        walls, pending = {seed}, [seed]
        while pending:
            node = pending.pop()
            for neighbour in graph.neighbours(node):
                if neighbour in remaining and cylinders[seed].same_surface(cylinders[neighbour]):
                    remaining.remove(neighbour)
                    walls.add(neighbour)
                    pending.append(neighbour)
        try:
            proof = _prove(graph, tuple(sorted(walls, key=lambda n: n.index)), cylinders[seed])
        except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
            continue
        if proof is not None:
            proofs.append(proof)
    return tuple(proofs)
