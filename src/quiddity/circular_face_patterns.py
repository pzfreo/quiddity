# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Prove rotational repetition of connected source-face groups.

The comparison is made over sampled source surfaces, not face counts: STEP can split the same
blade differently at a seam. The result names geometry and a bounded fit error. Exact source-face
membership is retained in candidate evidence and projected to document-local indices.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

from build123d import GeomType

from quiddity._adjacency import FaceGraph, FaceNode, SolidRef
from quiddity._candidates import CompletedInputs, FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._definitions import (
    DiscoveryServices,
    FullyAttributed,
    ManifestEvidence,
    NotCounted,
    PhysicalDefinition,
    always,
)
from quiddity._geometry import COORD_FLOOR, cross, dot, part_scale, unit
from quiddity._record import Record
from quiddity._typing import FaceLike, Part, Vector3

_MIN_COUNT = 3
_MAX_COUNT = 16
_MAX_FACES = 300
_MAX_MESH_POINTS = 1_000_000
_MESH_DEFLECTION_FRAC = 0.0025
_MATCH_DEFLECTIONS = 2.5
_GROUP_AREA_FRAC = 0.005
_CENTRAL_CENTRE_FRAC = 0.01
_AXIS_LINE_FRAC = 0.0001


@dataclass(frozen=True, order=True)
class CircularFacePattern(Record):
    """One repeated connected surface group on one source solid.

    ``fit_error`` is the largest bidirectional 99th-percentile distance between sampled
    corresponding surface groups after one pitch rotation, in model units. It is a bounded
    geometric fit, not a promise of identical STEP face decomposition.
    """

    axis_origin: Vector3
    axis_direction: Vector3
    count: int
    pitch_degrees: float
    seed_index: int
    fit_error: float


@dataclass(frozen=True, slots=True)
class _Axis:
    origin: Vector3
    direction: Vector3
    score: float


@dataclass(frozen=True, slots=True)
class _FaceFact:
    node: FaceNode
    face: FaceLike
    centre: Vector3
    area: float
    kind: str
    axis: _Axis | None


@dataclass(frozen=True, slots=True)
class _Proposal:
    record: CircularFacePattern
    groups: tuple[frozenset[FaceNode], ...]


def _vector(value: object) -> Vector3:
    return (float(value.X), float(value.Y), float(value.Z))  # type: ignore[attr-defined]


def _subtract(left: Vector3, right: Vector3) -> Vector3:
    return (left[0] - right[0], left[1] - right[1], left[2] - right[2])


def _add(left: Vector3, right: Vector3) -> Vector3:
    return (left[0] + right[0], left[1] + right[1], left[2] + right[2])


def _scale(value: Vector3, factor: float) -> Vector3:
    return (value[0] * factor, value[1] * factor, value[2] * factor)


def _length(value: Vector3) -> float:
    return math.sqrt(dot(value, value))


def _face_axis(face: FaceLike) -> _Axis | None:
    if face.geom_type not in (GeomType.CYLINDER, GeomType.TORUS):
        return None
    try:
        source = face.axis_of_rotation
        if source is None:
            return None
        direction = unit(_vector(source.direction))
        if direction[max(range(3), key=lambda i: abs(direction[i]))] < 0:
            direction = _scale(direction, -1)
        position = _vector(source.position)
    except (AttributeError, RuntimeError, ValueError, ZeroDivisionError):
        return None
    return _Axis(position, direction, float(face.area))


def _same_axis(left: _Axis, right: _Axis, *, scale: float) -> bool:
    return dot(left.direction, right.direction) > 1 - 1e-8 and _length(
        cross(_subtract(left.origin, right.origin), left.direction)
    ) <= max(COORD_FLOOR, scale * _AXIS_LINE_FRAC)


def _axes(facts: tuple[_FaceFact, ...], centre: Vector3, *, scale: float) -> tuple[_Axis, ...]:
    merged: list[_Axis] = []
    for fact in facts:
        if fact.axis is None:
            continue
        axis = fact.axis
        match = next(
            (i for i, item in enumerate(merged) if _same_axis(item, axis, scale=scale)), None
        )
        if match is None:
            merged.append(axis)
        else:
            previous = merged[match]
            merged[match] = _Axis(previous.origin, previous.direction, previous.score + axis.score)
    ranked = sorted(merged, key=lambda item: -item.score)[:4]
    if not ranked:
        ranked = [
            _Axis(centre, direction, 0.0)
            for direction in ((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0))
        ]
    return tuple(
        _Axis(
            _add(
                axis.origin,
                _scale(axis.direction, dot(_subtract(centre, axis.origin), axis.direction)),
            ),
            axis.direction,
            axis.score,
        )
        for axis in ranked
    )


def _basis(direction: Vector3) -> tuple[Vector3, Vector3]:
    smallest = min(range(3), key=lambda j: abs(direction[j]))
    reference: Vector3 = (float(smallest == 0), float(smallest == 1), float(smallest == 2))
    first = unit(cross(direction, reference))
    return first, cross(direction, first)


def _angle(centre: Vector3, axis: _Axis, basis: tuple[Vector3, Vector3]) -> float:
    offset = _subtract(centre, axis.origin)
    return math.atan2(dot(offset, basis[1]), dot(offset, basis[0])) % (2 * math.pi)


def _connected(group: frozenset[FaceNode], graph: FaceGraph) -> bool:
    reached = {next(iter(group))}
    pending = list(reached)
    while pending:
        for neighbour in graph.neighbours(pending.pop()):
            if neighbour in group and neighbour not in reached:
                reached.add(neighbour)
                pending.append(neighbour)
    return reached == group


def _groups(
    facts: tuple[_FaceFact, ...],
    graph: FaceGraph,
    axis: _Axis,
    *,
    count: int,
    excluded: frozenset[FaceNode],
) -> tuple[tuple[frozenset[FaceNode], ...], ...]:
    moving = tuple(fact for fact in facts if fact.node not in excluded)
    if len(moving) < count * 2:
        return ()
    pitch = 2 * math.pi / count
    basis = _basis(axis.direction)
    angles = {fact.node: _angle(fact.centre, axis, basis) for fact in moving}
    residues = sorted({angle % pitch for angle in angles.values()})
    phases = tuple(
        (value + following) / 2
        for value, following in zip(residues, (*residues[1:], residues[0] + pitch), strict=True)
    )
    by_node = {fact.node: fact for fact in moving}
    seen: set[tuple[tuple[int, ...], ...]] = set()
    candidates: list[tuple[frozenset[FaceNode], ...]] = []
    for phase in phases:
        groups = tuple(
            frozenset(
                node
                for node, angle in angles.items()
                if int(((angle - phase) % (2 * math.pi)) / pitch) == index
            )
            for index in range(count)
        )
        if any(len(group) < 2 for group in groups):
            continue
        identity = tuple(tuple(sorted(node.index for node in group)) for group in groups)
        if identity in seen:
            continue
        seen.add(identity)
        areas = tuple(sum(by_node[node].area for node in group) for group in groups)
        mean = sum(areas) / count
        if any(abs(area - mean) > mean * _GROUP_AREA_FRAC for area in areas):
            continue
        kinds = tuple(
            {
                kind: sum(by_node[node].area for node in group if by_node[node].kind == kind)
                for kind in {by_node[node].kind for node in group}
            }
            for group in groups
        )
        if any(
            abs(first.get(kind, 0.0) - other.get(kind, 0.0)) > mean * _GROUP_AREA_FRAC
            for other in kinds[1:]
            for first in kinds[:1]
            for kind in first.keys() | other.keys()
        ):
            continue
        if all(_connected(group, graph) for group in groups):
            candidates.append(groups)
    return tuple(candidates)


def _rotated(points: object, axis: _Axis, angle: float) -> object:
    import numpy as np

    origin = np.asarray(axis.origin)
    direction = np.asarray(axis.direction)
    offset = points - origin
    parallel = np.outer(offset @ direction, direction)
    transverse = offset - parallel
    return (
        origin
        + parallel
        + math.cos(angle) * transverse
        + math.sin(angle) * np.cross(direction, transverse)
    )


def _distance(points: object, target: object) -> float:
    import numpy as np
    from scipy.spatial import cKDTree  # type: ignore[import-untyped]

    forward = cKDTree(target).query(points, workers=1)[0]
    backward = cKDTree(points).query(target, workers=1)[0]
    return float(max(np.quantile(forward, 0.99), np.quantile(backward, 0.99)))


def _recognise_solid(
    graph: FaceGraph,
    solid: Part,
    nodes: tuple[FaceNode, ...],
) -> _Proposal | None:
    if len(nodes) < _MIN_COUNT * 2 or len(nodes) > _MAX_FACES:
        return None
    bounds = graph.solid_properties.bounding_box(solid)
    scale = part_scale(bounds)
    if scale <= COORD_FLOOR:
        return None
    mesh_deflection = scale * _MESH_DEFLECTION_FRAC
    facts = tuple(
        _FaceFact(
            node,
            graph.face(node),
            _vector(graph.face(node).center()),
            float(graph.face(node).area),
            graph.face(node).geom_type.name,
            _face_axis(graph.face(node)),
        )
        for node in nodes
    )
    axes = _axes(facts, _vector(bounds.center()), scale=scale)
    clouds: dict[FaceNode, object] = {}
    point_count = 0
    all_meshed = False

    def cloud(fact: _FaceFact) -> object:
        nonlocal point_count
        import numpy as np

        existing = clouds.get(fact.node)
        if existing is not None:
            return existing
        vertices, _triangles = fact.face.tessellate(mesh_deflection)
        points = np.asarray([_vector(vertex) for vertex in vertices])
        point_count += len(points)
        if point_count > _MAX_MESH_POINTS:
            raise ValueError("circular pattern mesh budget exceeded")
        clouds[fact.node] = points
        return points

    for axis in axes:
        for count in range(_MAX_COUNT, _MIN_COUNT - 1, -1):
            pitch = 2 * math.pi / count
            possible_background = tuple(
                fact
                for fact in facts
                if _length(cross(_subtract(fact.centre, axis.origin), axis.direction))
                <= scale * _CENTRAL_CENTRE_FRAC
                or (fact.axis is not None and _same_axis(fact.axis, axis, scale=scale))
            )
            try:
                excluded = frozenset(
                    fact.node
                    for fact in possible_background
                    if _distance(_rotated(cloud(fact), axis, pitch), cloud(fact))
                    <= mesh_deflection * _MATCH_DEFLECTIONS
                )
                by_node = {fact.node: fact for fact in facts}
                import numpy as np

                for groups in _groups(facts, graph, axis, count=count, excluded=excluded):
                    if not all_meshed:
                        for fact in facts:
                            cloud(fact)
                        all_meshed = True
                    group_clouds = tuple(
                        np.concatenate(
                            [cloud(by_node[node]) for node in sorted(group, key=lambda n: n.index)]
                        )
                        for group in groups
                    )
                    errors = tuple(
                        _distance(
                            _rotated(group_clouds[index], axis, pitch),
                            group_clouds[(index + 1) % count],
                        )
                        for index in range(count)
                    )
                    fit_error = max(errors)
                    if fit_error <= mesh_deflection * _MATCH_DEFLECTIONS:
                        record = CircularFacePattern(
                            axis_origin=(
                                round(axis.origin[0], 6),
                                round(axis.origin[1], 6),
                                round(axis.origin[2], 6),
                            ),
                            axis_direction=(
                                round(axis.direction[0], 9),
                                round(axis.direction[1], 9),
                                round(axis.direction[2], 9),
                            ),
                            count=count,
                            pitch_degrees=round(math.degrees(pitch), 9),
                            seed_index=0,
                            fit_error=round(fit_error, 6),
                        )
                        return _Proposal(record, groups)
            except (RuntimeError, ValueError, ZeroDivisionError):
                continue
    return None


def _discover_circular_face_patterns(
    part: Part, *, writer: EvidenceWriter | None = None
) -> list[CircularFacePattern]:
    graph = writer.graph if writer is not None else FaceGraph(part)
    by_solid: dict[SolidRef, list[FaceNode]] = {}
    for node in graph.nodes:
        owner = graph.common_valid_solid((node,))
        if owner is not None:
            by_solid.setdefault(owner, []).append(node)
    proposals = [
        proposal
        for owner, nodes in by_solid.items()
        if (proposal := _recognise_solid(graph, graph.solid_shape(owner), tuple(nodes))) is not None
    ]
    proposals.sort(key=lambda item: item.record)
    if writer is not None:
        for proposal in proposals:
            members = frozenset().union(*proposal.groups)
            writer.add_defining(
                proposal.record,
                members,
                constituent=members,
                groups=proposal.groups,
                family=FamilyId.CIRCULAR_FACE_PATTERNS,
            )
    return [proposal.record for proposal in proposals]


def recognise_circular_face_patterns(part: Part) -> list[CircularFacePattern]:
    """Return rotationally repeated connected face groups, without naming their CAD feature."""

    return _discover_circular_face_patterns(part)


def _discover(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    del inputs
    return list(_discover_circular_face_patterns(services.context.part, writer=services.writer))


DEFINITION = PhysicalDefinition(
    family=FamilyId.CIRCULAR_FACE_PATTERNS,
    record_types=(CircularFacePattern,),
    result_field="circular_face_patterns",
    public_entrypoint=recognise_circular_face_patterns.__name__,
    dependencies=(),
    applicable=always,
    discover=_discover,
    census=NotCounted("repeated face groups are correspondence evidence, not new features"),
    attribution=FullyAttributed("each instance retains its exact connected source-face group"),
    evidence=ManifestEvidence(
        golden_paths=("tests/circular_face_pattern_expected.json",),
        tests=("tests/test_circular_face_patterns.py",),
        introduced="0.3.6",
    ),
)


__all__ = ["CircularFacePattern", "recognise_circular_face_patterns"]
