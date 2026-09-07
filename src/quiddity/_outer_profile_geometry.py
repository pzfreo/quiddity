# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Original-wire inspection behind the run-local evidence facade (ADR 0025)."""

from __future__ import annotations

import math
from dataclasses import replace

from build123d import Edge, GeomType, Vertex
from OCP.BRepTools import BRepTools_WireExplorer
from OCP.TopExp import TopExp

from quiddity._adjacency import FaceGraph, FaceNode
from quiddity._outer_profile import (
    OuterProfileRefusalReason as Reason,
)
from quiddity._outer_profile import (
    PlanarOuterProfile,
    Point3,
    ProfileArc,
    ProfileLine,
    RefusedPlanarOuterProfile,
    _cross,
    _dot,
    _sub,
    _turns,
)
from quiddity._typing import FaceLike

# Original analytic support coincidence, in model length units (normally mm),
# consistent with ADR0008's 1e-6 source endpoint bound. No public rounding is used.
_POSITION_TOL = 1e-6
_DIRECTION_TOL = 2e-8


def _read_profile(
    face: FaceLike,
) -> tuple[PlanarOuterProfile, tuple[Edge, ...]] | RefusedPlanarOuterProfile:
    if face.geom_type != GeomType.PLANE:
        return RefusedPlanarOuterProfile(Reason.NOT_PLANAR)
    inner_loop_count = len(face.inner_wires())
    wire = face.outer_wire()
    explorer = BRepTools_WireExplorer(wire.wrapped, face.wrapped)
    edges: list[Edge] = []
    vertices: list[Vertex] = []
    while explorer.More():
        edge = Edge(explorer.Current())
        vertex = Vertex(explorer.CurrentVertex())
        if not vertex.wrapped.IsSame(TopExp.FirstVertex_s(edge.wrapped, True)):
            return RefusedPlanarOuterProfile(Reason.INVALID_BOUNDARY)
        edges.append(edge)
        vertices.append(vertex)
        explorer.Next()
    if not edges or len(edges) != len(wire.edges()):
        return RefusedPlanarOuterProfile(Reason.INVALID_BOUNDARY)
    if any(edge.geom_type not in (GeomType.LINE, GeomType.CIRCLE) for edge in edges):
        return RefusedPlanarOuterProfile(Reason.UNSUPPORTED_CURVE)
    if sum(edge.geom_type == GeomType.LINE for edge in edges) < 2:
        return RefusedPlanarOuterProfile(Reason.INSUFFICIENT_LINE_SUPPORTS)
    normal: Point3 = tuple(face.normal_at())
    points: list[Point3] = [tuple(vertex.center()) for vertex in vertices]
    origin = points[0]
    supports: list[ProfileLine | ProfileArc] = []
    for at, edge in enumerate(edges):
        following = (at + 1) % len(edges)
        if not TopExp.LastVertex_s(edge.wrapped, True).IsSame(vertices[following].wrapped):
            return RefusedPlanarOuterProfile(Reason.INVALID_BOUNDARY)
        start, end = points[at], points[following]
        if math.dist(start, end) == 0:
            return RefusedPlanarOuterProfile(Reason.INVALID_BOUNDARY)
        # OCCT can accept a vertex displaced from its trimmed curve endpoint
        # within a larger imported topology tolerance. Using that vertex would
        # invent a different finite support even though the wire stays connected.
        curve_ends = (tuple(edge.position_at(0)), tuple(edge.position_at(1)))
        if (
            min(
                max(math.dist(start, a), math.dist(end, b))
                for a, b in (curve_ends, curve_ends[::-1])
            )
            > _POSITION_TOL
        ):
            return RefusedPlanarOuterProfile(Reason.INVALID_BOUNDARY)
        midpoint: Point3 = tuple(edge.position_at(0.5))
        if any(
            abs(_dot(_sub(point, origin), normal)) > _POSITION_TOL
            for point in (start, end, midpoint)
        ):
            return RefusedPlanarOuterProfile(Reason.INVALID_BOUNDARY)
        if edge.geom_type == GeomType.LINE:
            line = ProfileLine(start, end)
            source_direction = ProfileLine(*curve_ends).direction
            if (
                min(
                    math.dist(line.direction, source_direction),
                    math.dist(line.direction, tuple(-v for v in source_direction)),
                )
                > _DIRECTION_TOL
            ):
                return RefusedPlanarOuterProfile(Reason.INVALID_BOUNDARY)
            supports.append(line)
        else:
            center: Point3 = tuple(edge.arc_center)
            sense = _dot(_cross(_sub(start, center), _sub(midpoint, center)), normal)
            if sense == 0:
                return RefusedPlanarOuterProfile(Reason.INVALID_BOUNDARY)
            supports.append(
                ProfileArc(
                    start, end, center, edge.radius, math.copysign(edge.length / edge.radius, sense)
                )
            )

    # OCCT wire orientation can differ from outward face orientation. Choose the
    # complete boundary winding, not an individual edge's parameter direction.
    turns = _turns(supports, normal)
    winding = math.fsum(turns) + math.fsum(s.sweep for s in supports if isinstance(s, ProfileArc))
    if winding < 0:
        supports = [
            replace(s, start=s.end, end=s.start, sweep=-s.sweep)
            if isinstance(s, ProfileArc)
            else replace(s, start=s.end, end=s.start)
            for s in reversed(supports)
        ]
        edges.reverse()
        turns = _turns(supports, normal)
        winding = -winding
    if abs(winding - 2 * math.pi) > _DIRECTION_TOL:
        return RefusedPlanarOuterProfile(Reason.INVALID_BOUNDARY)
    if any(turn < -_DIRECTION_TOL for turn in turns) or any(
        isinstance(s, ProfileArc) and s.sweep < 0 for s in supports
    ):
        return RefusedPlanarOuterProfile(Reason.CONCAVE_PROFILE)
    first = min(range(len(supports)), key=lambda at: supports[at].start)
    supports = supports[first:] + supports[:first]
    edges = edges[first:] + edges[:first]
    return PlanarOuterProfile(supports[0].start, normal, tuple(supports), inner_loop_count), tuple(
        edges
    )


class _OuterProfileSource:
    """Lazy per-face facts on the existing graph; no recognition or attribution pass."""

    def __init__(self, graph: FaceGraph):
        self.graph = graph
        self.body_nodes: dict[object, frozenset[FaceNode]] = {}

    def read(self, node: FaceNode):
        owner = self.graph.common_valid_solid((node,))
        if owner is None:
            return RefusedPlanarOuterProfile(Reason.AMBIGUOUS_BODY)
        if owner not in self.body_nodes:
            # Exact graph ownership: equal geometry or touching bodies cannot join.
            nodes = frozenset(
                self.graph.require_node(face) for face in self.graph.solid_shape(owner).faces()
            )
            if self.graph.common_valid_solid(nodes) is not owner:
                return RefusedPlanarOuterProfile(Reason.AMBIGUOUS_BODY)
            self.body_nodes[owner] = nodes
        try:
            value = _read_profile(self.graph.face(node))
        except (ValueError, RuntimeError):
            return RefusedPlanarOuterProfile(Reason.INVALID_BOUNDARY)
        if isinstance(value, RefusedPlanarOuterProfile):
            return value
        profile, edges = value
        return profile, edges, self.body_nodes[owner]
