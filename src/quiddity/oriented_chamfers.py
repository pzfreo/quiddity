# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""External planar chamfers on edges oblique to the supplied principal frame.

The bevel must replace one continuous straight, convex edge span of one valid solid. Its
two planar support faces meet at a virtual right-angle edge; the reported legs are
measured along those supports, rather than projected onto caller XYZ. Concave bevels,
split faces and unproved support geometry are outside this conservative subset.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from functools import total_ordering
from typing import Any

from build123d import GeomType, Vector

from quiddity._adjacency import FaceGraph, FaceNode
from quiddity._bevel import _material_at
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
from quiddity._geometry import AXIS_ALIGNED_COS, COORD_FLOOR, INTERIOR_PROBE_FRAC, SMOOTH_ARC_GAP
from quiddity._record import Record
from quiddity._typing import Part


@total_ordering
@dataclass(frozen=True)
class OrientedChamfer(Record):
    """One measured external oblique-edge bevel in caller coordinates.

    ``run`` follows the lexicographic direction of the original support edges;
    ``length`` is their common run span. ``corner`` is the virtual sharp edge at
    the midpoint of that common span, and ``at`` is the centre of the bevel face.
    Positive ``leg1`` and ``leg2`` run from ``corner`` to the bevel's two long edges
    along their respective support planes. The larger leg is first; equal legs use
    lexicographic direction order. ``angle`` is atan2(leg2, leg1) in degrees.
    The directions, like ``run``, are unit vectors in the supplied part frame.
    ``support_spans`` gives each original long edge's start/end run distances
    relative to ``corner``, in leg order, so clipped ends are not made rectangular.
    """

    run: tuple[float, float, float]
    length: float
    at: tuple[float, float, float]
    corner: tuple[float, float, float]
    leg1: float
    leg2: float
    leg1_direction: tuple[float, float, float]
    leg2_direction: tuple[float, float, float]
    support_spans: tuple[tuple[float, float], tuple[float, float]]
    angle: float
    body_key: BodyKey | None = ()

    def __lt__(self, other: object) -> bool:
        if not isinstance(other, OrientedChamfer):
            return NotImplemented
        return (
            self.run,
            self.length,
            self.at,
            self.corner,
            self.leg1,
            self.leg2,
            self.leg1_direction,
            self.leg2_direction,
            self.support_spans,
            self.angle,
            self.body_key is not None,
            self.body_key or (),
        ) < (
            other.run,
            other.length,
            other.at,
            other.corner,
            other.leg1,
            other.leg2,
            other.leg1_direction,
            other.leg2_direction,
            other.support_spans,
            other.angle,
            other.body_key is not None,
            other.body_key or (),
        )


def _point(vertex: Any) -> Vector:
    return Vector(vertex.X, vertex.Y, vertex.Z)


def _coordinates(point: Vector, digits: int) -> tuple[float, float, float]:
    def rounded(value: float) -> float:
        result = round(value, digits)
        return result if result else 0.0

    return (rounded(point.X), rounded(point.Y), rounded(point.Z))


def _linear_quad(face: Any) -> bool:
    return (
        face.geom_type == GeomType.PLANE
        and len(face.wires()) == 1
        and len(face.outer_wire().edges()) == 4
        and all(edge.geom_type == GeomType.LINE for edge in face.outer_wire().edges())
        and len(face.vertices()) == 4
    )


def _edge_info(graph: FaceGraph, bevel: FaceNode, neighbour: FaceNode) -> tuple[Any, Vector] | None:
    if graph.arc(bevel, neighbour) == "convex":
        face = graph.face(neighbour)
        if face.geom_type != GeomType.PLANE:
            return None
        edges = graph.shared_edges(bevel, neighbour)
        if len(edges) != 1 or edges[0].geom_type != GeomType.LINE:
            return None
        return edges[0], face.normal_at().normalized()
    return None


def _pair(
    solid: Any,
    graph: FaceGraph,
    bevel: FaceNode,
    left: FaceNode,
    right: FaceNode,
    body_key: BodyKey | None,
    max_leg_frac: float,
    stock_size: float,
    box: Any,
) -> OrientedChamfer | None:
    lhs = _edge_info(graph, bevel, left)
    rhs = _edge_info(graph, bevel, right)
    if lhs is None or rhs is None:
        return None
    edge1, normal1 = lhs
    edge2, normal2 = rhs
    if abs(normal1.dot(normal2)) > SMOOTH_ARC_GAP:
        return None
    ends1 = sorted((_point(vertex) for vertex in edge1.vertices()), key=lambda p: (p.X, p.Y, p.Z))
    ends2 = sorted((_point(vertex) for vertex in edge2.vertices()), key=lambda p: (p.X, p.Y, p.Z))
    if len(ends1) != 2 or len(ends2) != 2:
        return None
    span1 = ends1[1] - ends1[0]
    span2 = ends2[1] - ends2[0]
    if min(span1.length, span2.length) <= COORD_FLOOR:
        return None
    run = span1.normalized()
    if run.dot(span2.normalized()) < 1 - SMOOTH_ARC_GAP:
        return None
    if max(abs(run.X), abs(run.Y), abs(run.Z)) >= AXIS_ALIGNED_COS:
        return None
    if abs(normal1.dot(run)) > SMOOTH_ARC_GAP or abs(normal2.dot(run)) > SMOOTH_ARC_GAP:
        return None
    # The two support edges must be opposite, not two sides of one triangular end.
    if min((a - b).length for a in ends1 for b in ends2) <= COORD_FLOOR:
        return None
    origin = ends1[0]
    first_span = (0.0, span1.length)
    second_span = tuple(sorted((point - origin).dot(run) for point in ends2))
    common_start = max(first_span[0], second_span[0])
    common_end = min(first_span[1], second_span[1])
    length = common_end - common_start
    if length <= COORD_FLOOR:
        return None
    station = (common_start + common_end) * 0.5
    face = graph.face(bevel)
    surface_normal = face.normal_at().normalized()
    if min(surface_normal.dot(normal1), surface_normal.dot(normal2)) <= SMOOTH_ARC_GAP:
        # A replacement bevel faces out between its two support normals. An
        # ordinary side wall meeting a top and end face is not such a bevel.
        return None
    at = face.center()
    cross = at + run * (station - (at - origin).dot(run))
    mid1 = (ends1[0] + ends1[1]) * 0.5
    mid2 = (ends2[0] + ends2[1]) * 0.5
    mid1 += run * (station - (mid1 - origin).dot(run))
    mid2 += run * (station - (mid2 - origin).dot(run))
    dot = normal1.dot(normal2)
    denominator = 1 - dot * dot
    offset1 = (mid1 - cross).dot(normal1)
    offset2 = (mid2 - cross).dot(normal2)
    corner = (
        cross
        + normal1 * ((offset1 - dot * offset2) / denominator)
        + normal2 * ((offset2 - dot * offset1) / denominator)
    )
    leg_vec1 = mid1 - corner
    leg_vec2 = mid2 - corner
    leg1 = leg_vec1.length
    leg2 = leg_vec2.length
    if min(leg1, leg2) <= COORD_FLOOR or max(leg1, leg2) > max_leg_frac * stock_size:
        return None
    # A broad terminal face between two real bevel strips can imitate a bevel.
    # Its reconstructed sharp edge lies beyond this solid's bounding envelope;
    # a proved local edge break must stay within that same body's envelope.
    envelope_tol = max(COORD_FLOOR, stock_size * 1e-6)
    if any(
        coordinate < lower - envelope_tol or coordinate > upper + envelope_tol
        for coordinate, lower, upper in zip(
            (corner.X, corner.Y, corner.Z),
            (box.min.X, box.min.Y, box.min.Z),
            (box.max.X, box.max.Y, box.max.Z),
            strict=True,
        )
    ):
        return None
    if abs(leg_vec1.normalized().dot(leg_vec2.normalized())) > SMOOTH_ARC_GAP:
        return None
    # The corner-to-face side is removed material. A probe on the other side of
    # the sharp edge must also be empty: a concave gusset or web fails this proof.
    toward = corner + (cross - corner) * INTERIOR_PROBE_FRAC
    beyond = corner - (cross - corner) * INTERIOR_PROBE_FRAC
    if _material_at(solid, (toward.X, toward.Y, toward.Z), properties=graph.solid_properties):
        return None
    if _material_at(solid, (beyond.X, beyond.Y, beyond.Z), properties=graph.solid_properties):
        return None
    offset = min(leg1, leg2) * INTERIOR_PROBE_FRAC
    inside = cross - surface_normal * offset
    outside = cross + surface_normal * offset
    if not _material_at(solid, (inside.X, inside.Y, inside.Z), properties=graph.solid_properties):
        return None
    if _material_at(solid, (outside.X, outside.Y, outside.Z), properties=graph.solid_properties):
        return None
    legs = sorted(
        (
            (leg1, leg_vec1.normalized(), first_span),
            (leg2, leg_vec2.normalized(), second_span),
        ),
        key=lambda item: (-round(item[0], 6), _coordinates(item[1], 6)),
    )
    return OrientedChamfer(
        run=_coordinates(run, 6),
        length=round(length, 3),
        at=_coordinates(at, 3),
        corner=_coordinates(corner, 3),
        leg1=round(legs[0][0], 3),
        leg2=round(legs[1][0], 3),
        leg1_direction=_coordinates(legs[0][1], 6),
        leg2_direction=_coordinates(legs[1][1], 6),
        support_spans=(
            (round(legs[0][2][0] - station, 3), round(legs[0][2][1] - station, 3)),
            (round(legs[1][2][0] - station, 3), round(legs[1][2][1] - station, 3)),
        ),
        angle=round(math.degrees(math.atan2(legs[1][0], legs[0][0])), 2),
        body_key=body_key,
    )


def _discover_oriented_chamfers(
    part: Part, *, graph: FaceGraph, sink: EvidenceSink | None, max_leg_frac: float = 0.45
) -> list[OrientedChamfer]:
    if not 0 < max_leg_frac < 1:
        raise ValueError("max_leg_frac must be between zero and one")
    solids = list(part.solids())
    keys = unambiguous_body_keys(
        solids, require_valid_solid=True, properties=graph.solid_properties
    )
    proposals: list[tuple[OrientedChamfer, FaceNode]] = []
    for solid, body_key in zip(solids, keys, strict=True):
        # Existing bevel families probe the supplied part. Reuse its cached
        # classifier for a one-body shape; scope probes to the solid in compounds.
        probe_shape = part if len(solids) == 1 else solid
        solid_nodes = {graph.require_node(face) for face in solid.faces()}
        box = graph.solid_properties.bounding_box(solid)
        stock_size = max(box.max.X - box.min.X, box.max.Y - box.min.Y, box.max.Z - box.min.Z)
        for bevel in sorted(solid_nodes, key=lambda node: node.index):
            face = graph.face(bevel)
            if not _linear_quad(face):
                continue
            neighbours = [node for node in graph.neighbours(bevel) if node in solid_nodes]
            if len(neighbours) != 4:
                continue
            if any(
                graph.face(node).geom_type == GeomType.PLANE
                and len(graph.face(node).outer_wire().edges()) == 3
                for node in neighbours
            ):
                # A triangular blind terminal makes this slant an angled step;
                # a raw oblique run cannot yet publish that step family.
                continue
            records = [
                record
                for i, left in enumerate(neighbours)
                for right in neighbours[i + 1 :]
                if (
                    record := _pair(
                        probe_shape,
                        graph,
                        bevel,
                        left,
                        right,
                        body_key,
                        max_leg_frac,
                        stock_size,
                        box,
                    )
                )
                is not None
            ]
            if len(records) != 1:
                continue
            if graph.common_valid_solid((bevel,)) is None:
                if graph.local_degradation:
                    continue
                raise ValueError("OrientedChamfer defining face does not belong to one valid solid")
            proposals.append((records[0], bevel))
    proposals.sort(key=lambda item: item[0])
    if sink is not None:
        for record, node in proposals:
            sink.propose(FamilyId.ORIENTED_CHAMFERS, record, defining=(node,))
    return [record for record, _node in proposals]


def recognise_oriented_chamfers(part: Part, *, max_leg_frac: float = 0.45) -> list[OrientedChamfer]:
    """Recognise external oblique-edge chamfers with a body-local corner proof."""

    return _discover_oriented_chamfers(
        part, graph=FaceGraph(part), sink=None, max_leg_frac=max_leg_frac
    )


def _discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    del inputs
    return list(
        _discover_oriented_chamfers(
            services.context.part, graph=services.writer.graph, sink=services.writer.sink
        )
    )


DEFINITION = PhysicalDefinition(
    family=FamilyId.ORIENTED_CHAMFERS,
    record_types=(OrientedChamfer,),
    result_field="oriented_chamfers",
    public_entrypoint=recognise_oriented_chamfers.__name__,
    dependencies=(),
    applicable=prismatic,
    discover=_discover,
    census=Counted("oriented_chamfer"),
    attribution=FullyAttributed(
        "the bevel face and its convex support joins establish the edge break"
    ),
    evidence=ManifestEvidence(
        goldens=("oriented_chamfer",),
        tests=("tests/test_oriented_chamfers.py",),
        introduced="0.3.9",
    ),
)
