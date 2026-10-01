# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Through steps whose run is oblique in the caller's principal frame.

The supported profile has one principal planar wall and one rectangular oblique planar wall.
Their entire concave seam reaches proved ends of the source solid. A swept copy of the
principal wall is empty of material, and both seam ends have convex common terminals.
Fragmented, curved and locally terminated variants remain unclaimed until their full
boundary can be proved. The principal ThroughStep record retains its original axis and
section contract.
"""

from __future__ import annotations

from dataclasses import dataclass
from functools import total_ordering
from typing import Any

from build123d import GeomType, Pos, Solid, Vector

from quiddity._adjacency import FaceGraph, FaceNode, axis_aligned_axis
from quiddity._body_identity import BodyKey, unambiguous_body_keys
from quiddity._candidates import CompletedInputs, EvidenceSink, FamilyId
from quiddity._definitions import (
    Counted,
    DiscoveryServices,
    FullyAttributed,
    ManifestEvidence,
    PhysicalDefinition,
    prismatic,
)
from quiddity._geometry import COORD_FLOOR, SMOOTH_ARC_GAP, length_tol
from quiddity._record import Record
from quiddity._typing import Part
from quiddity._volume_probe import probe_volume

_COORDS = "XYZ"
_END_EPS = COORD_FLOOR


@total_ordering
@dataclass(frozen=True)
class ObliqueThroughStep(Record):
    """A complete, straight oblique-run open step in caller coordinates.

    ``at`` is the seam midpoint. ``run`` points from the lexicographically first seam
    endpoint to the second, and ``length`` is the complete seam length. ``depth_direction``
    points from the principal wall into the removed volume; ``depth`` is the distance to the
    opposite boundary along it. ``across_direction`` points from the seam into the principal
    wall. The three unit vectors define the frame; ``wall_outline`` lists the four vertices
    of that wall as ``(run, across)`` distances from the first seam endpoint, in boundary
    order. Its variable outline records clipped trapezoids without inventing a constant
    cross section. ``body_key`` identifies the same valid source solid as other body records.
    """

    run: tuple[float, float, float]
    length: float
    at: tuple[float, float, float]
    depth_direction: tuple[float, float, float]
    depth: float
    across_direction: tuple[float, float, float]
    wall_outline: tuple[
        tuple[float, float],
        tuple[float, float],
        tuple[float, float],
        tuple[float, float],
    ]
    body_key: BodyKey | None = ()

    def __lt__(self, other: object) -> bool:
        if not isinstance(other, ObliqueThroughStep):
            return NotImplemented
        return (
            self.run,
            self.length,
            self.at,
            self.depth_direction,
            self.depth,
            self.across_direction,
            self.wall_outline,
            self.body_key is not None,
            self.body_key or (),
        ) < (
            other.run,
            other.length,
            other.at,
            other.depth_direction,
            other.depth,
            other.across_direction,
            other.wall_outline,
            other.body_key is not None,
            other.body_key or (),
        )


def _point(vertex: Any) -> Vector:
    return Vector(vertex.X, vertex.Y, vertex.Z)


def _coordinates(point: Vector) -> tuple[float, float, float]:
    return (float(point.X), float(point.Y), float(point.Z))


def _rounded_vector(point: Vector, digits: int) -> tuple[float, float, float]:
    def rounded(value: float) -> float:
        result = round(value, digits)
        return result if result else 0.0

    return (rounded(point.X), rounded(point.Y), rounded(point.Z))


def _canonical_outline(
    vertices: list[tuple[float, float]],
) -> tuple[tuple[float, float], tuple[float, float], tuple[float, float], tuple[float, float]]:
    normalized = [(x if x else 0.0, y if y else 0.0) for x, y in vertices]
    paths = [normalized, list(reversed(normalized))]
    ordered = min(tuple(path[i:] + path[:i]) for path in paths for i in range(4))
    return (ordered[0], ordered[1], ordered[2], ordered[3])


def _bounds(box: Any) -> tuple[tuple[float, float], ...]:
    return tuple((getattr(box.min, c), getattr(box.max, c)) for c in _COORDS)


def _linear_quad(face: Any) -> bool:
    return (
        len(face.wires()) == 1
        and len(face.outer_wire().edges()) == 4
        and all(edge.geom_type == GeomType.LINE for edge in face.outer_wire().edges())
        and len(face.vertices()) == 4
    )


def _endpoint_on_envelope(
    point: Vector, bounds: tuple[tuple[float, float], ...], principal_axis: int
) -> bool:
    return any(
        abs(getattr(point, _COORDS[axis]) - bound) <= _END_EPS
        for axis in range(3)
        if axis != principal_axis
        for bound in bounds[axis]
    )


def _terminal(
    graph: FaceGraph,
    principal: FaceNode,
    oblique: FaceNode,
    endpoint: Vector,
    solid_nodes: set[FaceNode],
) -> bool:
    def convex_terminal_neighbours(source: FaceNode) -> set[FaceNode]:
        return {
            node
            for node in graph.neighbours(source)
            if node in solid_nodes
            and node not in (principal, oblique)
            and graph.arc(source, node) == "convex"
            and any(
                (_point(vertex) - endpoint).length <= _END_EPS
                for edge in graph.shared_edges(source, node)
                for vertex in edge.vertices()
            )
        }

    for left in convex_terminal_neighbours(principal):
        for right in convex_terminal_neighbours(oblique):
            if left is right:
                return True
            if graph.arc(left, right) == "convex" and any(
                (_point(vertex) - endpoint).length <= _END_EPS
                for edge in graph.shared_edges(left, right)
                for vertex in edge.vertices()
            ):
                return True
    return False


def _one_pair(
    solid: Any,
    graph: FaceGraph,
    principal: FaceNode,
    oblique: FaceNode,
    body_key: BodyKey | None,
    solid_bounds: tuple[tuple[float, float], ...],
    solid_nodes: set[FaceNode],
) -> ObliqueThroughStep | None:
    seam_arc = graph.arc(principal, oblique)
    if seam_arc != "concave":
        return None
    principal_face = graph.face(principal)
    oblique_face = graph.face(oblique)
    plane = axis_aligned_axis(principal_face.wrapped)
    if (
        plane is None
        or oblique_face.geom_type != GeomType.PLANE
        or axis_aligned_axis(oblique_face.wrapped) is not None
    ):
        return None
    if not _linear_quad(principal_face) or not _linear_quad(oblique_face):
        return None
    axis, coordinate = plane
    principal_normal = principal_face.normal_at().normalized()
    oblique_normal = oblique_face.normal_at().normalized()
    if abs(principal_normal.dot(oblique_normal)) > SMOOTH_ARC_GAP:
        return None
    shared = graph.shared_edges(principal, oblique)
    if len(shared) != 1 or shared[0].geom_type != GeomType.LINE:
        return None
    endpoints = [_point(vertex) for vertex in shared[0].vertices()]
    if len(endpoints) != 2:
        return None
    start, end = sorted(endpoints, key=_coordinates)
    run_length = (end - start).length
    if run_length <= COORD_FLOOR:
        return None
    run = (end - start).normalized()
    if (
        abs(run.dot(principal_normal)) > SMOOTH_ARC_GAP
        or abs(run.dot(oblique_normal)) > SMOOTH_ARC_GAP
    ):
        return None
    if not all(_endpoint_on_envelope(p, solid_bounds, axis) for p in (start, end)):
        return None
    if not all(_terminal(graph, principal, oblique, p, solid_nodes) for p in (start, end)):
        return None
    if not all(
        graph.arc(node, other) == "convex"
        for node, other_node in ((principal, oblique), (oblique, principal))
        for other in graph.neighbours(node)
        if other is not other_node
    ):
        return None
    # The oblique wall must be the four edges of the sweep's far side: one copy of
    # each seam endpoint on the principal wall, one at a common positive depth.
    oblique_vertices = [_point(vertex) for vertex in oblique_face.vertices()]
    depth_coordinates = sorted(
        {round((vertex - start).dot(principal_normal), 6) for vertex in oblique_vertices}
    )
    if len(depth_coordinates) != 2 or abs(depth_coordinates[0]) > _END_EPS:
        return None
    depth = depth_coordinates[1]
    if depth <= COORD_FLOOR:
        return None
    far = coordinate + depth * getattr(principal_normal, _COORDS[axis])
    if not any(abs(far - bound) <= _END_EPS for bound in solid_bounds[axis]):
        return None
    for vertex in oblique_vertices:
        offset = vertex - start
        if abs(offset.dot(oblique_normal)) > _END_EPS:
            return None
        if min(abs(offset.dot(run)), abs(offset.dot(run) - run_length)) > _END_EPS:
            return None
        if (
            min(abs(offset.dot(principal_normal)), abs(offset.dot(principal_normal) - depth))
            > _END_EPS
        ):
            return None
    principal_vertices = [_point(vertex) for vertex in principal_face.outer_wire().vertices()]
    if len(principal_vertices) != 4:
        return None
    outer = [
        vertex
        for vertex in principal_vertices
        if min((vertex - start).length, (vertex - end).length) > _END_EPS
    ]
    if len(outer) != 2:
        return None
    outer_edge = outer[1] - outer[0]
    if outer_edge.length <= COORD_FLOOR or not any(
        1 - abs(getattr(outer_edge.normalized(), _COORDS[c])) <= SMOOTH_ARC_GAP
        for c in range(3)
        if c != axis
    ):
        # A skew connecting edge closes a triangular blind-step wedge. A through
        # step's opposite wall boundary is a principal stock edge in this subset.
        return None
    across_values = [(vertex - start).dot(oblique_normal) for vertex in principal_vertices]
    if min(across_values) < -_END_EPS and max(across_values) > _END_EPS:
        return None
    across = oblique_normal if max(across_values) > _END_EPS else -oblique_normal
    outline = [
        (round((vertex - start).dot(run), 3), round((vertex - start).dot(across), 3))
        for vertex in principal_vertices
    ]
    if sum(value > _END_EPS for _station, value in outline) != 2:
        return None
    # A face-sweep follows the measured trapezoid exactly. Inset only along the
    # depth to prevent boundary contact from being counted as material.
    inset = min(length_tol(depth, rel=1e-6, floor=COORD_FLOOR), depth / 4)
    shifted = Pos(*_coordinates(principal_normal * inset)) * principal_face
    probe = Solid.extrude(shifted, principal_normal * (depth - 2 * inset))
    if probe.volume <= 0 or probe_volume(solid, probe, properties=graph) != 0.0:
        return None
    midpoint = (start + end) * 0.5
    return ObliqueThroughStep(
        _rounded_vector(run, 6),
        round(run_length, 3),
        _rounded_vector(midpoint, 3),
        _rounded_vector(principal_normal, 6),
        round(depth, 3),
        _rounded_vector(across, 6),
        _canonical_outline(outline),
        body_key,
    )


def _discover_oblique_through_steps(
    part: Part, *, graph: FaceGraph, sink: EvidenceSink | None
) -> list[ObliqueThroughStep]:
    solids = list(part.solids())
    keys = unambiguous_body_keys(
        solids, require_valid_solid=True, properties=graph.solid_properties
    )
    proposals: list[tuple[ObliqueThroughStep, tuple[FaceNode, FaceNode]]] = []
    for solid, key in zip(solids, keys, strict=True):
        solid_nodes = {graph.require_node(face) for face in solid.faces()}
        bounds = _bounds(graph.solid_properties.bounding_box(solid))
        for principal in sorted(solid_nodes, key=lambda node: node.index):
            if axis_aligned_axis(graph.face(principal).wrapped) is None:
                continue
            for oblique in graph.neighbours(principal):
                if oblique not in solid_nodes:
                    continue
                record = _one_pair(solid, graph, principal, oblique, key, bounds, solid_nodes)
                if record is None:
                    continue
                nodes = (principal, oblique)
                if graph.common_valid_solid(nodes) is None:
                    if graph.local_degradation:
                        continue
                    raise ValueError(
                        "ObliqueThroughStep defining faces do not belong to one valid solid"
                    )
                proposals.append((record, nodes))
    proposals.sort(key=lambda proposal: proposal[0])
    if sink is not None:
        for record, nodes in proposals:
            sink.propose(FamilyId.OBLIQUE_THROUGH_STEPS, record, defining=nodes)
    return [record for record, _nodes in proposals]


def recognise_oblique_through_steps(part: Part) -> list[ObliqueThroughStep]:
    """Recognise the proved two-wall oblique-run through-step subset."""

    return _discover_oblique_through_steps(part, graph=FaceGraph(part), sink=None)


def _discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    del inputs
    return list(
        _discover_oblique_through_steps(
            services.context.part, graph=services.writer.graph, sink=services.writer.sink
        )
    )


DEFINITION = PhysicalDefinition(
    family=FamilyId.OBLIQUE_THROUGH_STEPS,
    record_types=(ObliqueThroughStep,),
    result_field="oblique_through_steps",
    public_entrypoint=recognise_oblique_through_steps.__name__,
    dependencies=(),
    applicable=prismatic,
    discover=_discover,
    census=Counted("oblique_through_step"),
    attribution=FullyAttributed("both walls and their complete concave seam establish the step"),
    evidence=ManifestEvidence(
        goldens=("oblique_through_step",),
        tests=("tests/test_oblique_through_steps.py",),
        introduced="0.3.9",
    ),
)
