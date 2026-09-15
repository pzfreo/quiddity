# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Recognition of triangular ribs joining two perpendicular support planes."""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass, replace

from build123d import extrude
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Cylinder, GeomAbs_Plane

from quiddity._adjacency import (
    FaceEdges,
    FaceGraph,
    SolidRef,
    axis_aligned_axis,
    edge_face_map,
    neighbours,
)
from quiddity._body_identity import unambiguous_body_keys
from quiddity._candidates import EvidenceSink, FamilyId
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._definitions import (
    AcceptedInputs,
    Counted,
    DerivedDefinition,
    DerivedId,
    Evidence,
    FullyAttributed,
    NotCounted,
    PhysicalDefinition,
    prismatic,
    simple,
)
from quiddity._geometry import length_tol
from quiddity._pattern_geometry import _pattern_tol
from quiddity._record import Record
from quiddity._typing import FaceLike, Part


@dataclass(frozen=True, order=True)
class GussetRib(Record):
    """A right-triangular rib between two perpendicular support planes.

    ``thickness_axis`` and ``thickness_bounds`` locate the rib. ``supports`` lists the
    two axis/plane pairs at its square corner; ``legs`` gives the extent away from each
    corresponding plane, and ``directions`` says which signed side. The legs describe
    the virtual sharp profile when its slanted edges are filleted. ``body_key`` is
    an unambiguous result-local correlation signature for pattern membership. All
    coordinates use the supplied recognition frame.
    """

    thickness_axis: str
    thickness_bounds: tuple[float, float]
    supports: tuple[tuple[str, float], tuple[str, float]]
    legs: tuple[float, float]
    directions: tuple[int, int]
    # Equal signatures on separate solids are ambiguous; see Channel.body_key.
    body_key: tuple[float, ...] | None = ()


@dataclass(frozen=True)
class GussetRibMirrorPair(Record):
    """Two congruent ribs reflected across their body's centre plane."""

    ribs: tuple[GussetRib, GussetRib]
    mirror_plane: tuple[str, float]


@dataclass(frozen=True)
class GussetRibArray(Record):
    """At least three congruent ribs ordered at constant pitch along their thickness axis."""

    ribs: tuple[GussetRib, ...]
    axis: str
    pitch: float


@dataclass(frozen=True)
class _Cap:
    face: FaceLike
    axis: int
    at: float
    profile: tuple[tuple[float, float], ...]
    supports: tuple[FaceLike, FaceLike]
    support_coords: tuple[float, float]
    slant: FaceLike
    transition: FaceLike


def _xyz(vertex) -> tuple[float, float, float]:
    return tuple(float(value) for value in vertex)  # type: ignore[return-value]


def _cap(
    face: FaceLike,
    edge_faces: dict,
    graph: FaceGraph,
    face_edges: FaceEdges | None,
) -> _Cap | None:
    if BRepAdaptor_Surface(face.wrapped).GetType() != GeomAbs_Plane:
        return None
    aligned = axis_aligned_axis(face.wrapped)
    if aligned is None:
        return None
    axis = aligned[0]
    edges = face_edges.of(face) if face_edges is not None else face.edges()
    if len(edges) != 3 or any(edge.geom_type.name != "LINE" for edge in edges):
        return None
    vertices = tuple(_xyz(vertex) for vertex in face.vertices())
    if len(vertices) != 3:
        return None
    axes = tuple(i for i in range(3) if i != axis)
    tol = length_tol(min(edge.length for edge in edges), rel=1e-7)
    if max(vertex[axis] for vertex in vertices) - min(vertex[axis] for vertex in vertices) > tol:
        return None
    profile = tuple((vertex[axes[0]], vertex[axes[1]]) for vertex in vertices)
    square = next(
        (
            point
            for point in profile
            if any(abs(point[0] - other[0]) <= tol and other != point for other in profile)
            and any(abs(point[1] - other[1]) <= tol and other != point for other in profile)
        ),
        None,
    )
    if square is None:
        return None
    node = graph.require_node(face)
    supports: dict[int, FaceLike] = {}
    transition: FaceLike | None = None
    for edge in edges:
        incident = edge_faces.get(edge, ())
        if len(incident) != 2:
            return None
        other = next((item for item in incident if item != face), None)
        if other is None:
            return None
        endpoints = tuple(_xyz(vertex) for vertex in edge.vertices())
        if len(endpoints) != 2:
            return None
        fixed = next((i for i in axes if abs(endpoints[0][i] - endpoints[1][i]) <= tol), None)
        other_node = graph.require_node(other)
        if fixed is None:
            if transition is not None:
                return None
            transition = other
        else:
            support_plane = axis_aligned_axis(other.wrapped)
            support_is_concave = graph.arc(node, other_node) == "concave"
            if (
                fixed in supports
                or abs(endpoints[0][fixed] - square[axes.index(fixed)]) > tol
                or support_plane is None
                or support_plane[0] != fixed
                or abs(support_plane[1] - square[axes.index(fixed)]) > tol
                or not support_is_concave
            ):
                return None
            supports[fixed] = other
    if transition is None or set(supports) != set(axes):
        return None
    transition_node = graph.require_node(transition)
    surface = BRepAdaptor_Surface(transition.wrapped).GetType()
    if surface == GeomAbs_Plane and graph.arc(node, transition_node) == "convex":
        slant = transition
    elif surface == GeomAbs_Cylinder and graph.arc(node, transition_node) == "smooth":
        candidates = [
            other
            for other in neighbours(transition, edge_faces, face_edges=face_edges)
            if other != face
            and BRepAdaptor_Surface(other.wrapped).GetType() == GeomAbs_Plane
            and axis_aligned_axis(other.wrapped) is None
            and graph.arc(transition_node, graph.require_node(other)) == "smooth"
        ]
        if len(candidates) != 1:
            return None
        slant = candidates[0]
    else:
        return None
    return _Cap(
        face,
        axis,
        sum(vertex[axis] for vertex in vertices) / 3,
        tuple(sorted(profile)),
        (supports[axes[0]], supports[axes[1]]),
        square,
        slant,
        transition,
    )


def _discover_gusset_ribs(
    part: Part,
    *,
    graph: FaceGraph,
    face_edges: FaceEdges | None,
    sink: EvidenceSink | None,
) -> list[GussetRib]:
    faces = tuple(part.faces())
    edge_faces = edge_face_map(faces, face_edges=face_edges)
    caps = tuple(
        cap for face in faces if (cap := _cap(face, edge_faces, graph, face_edges)) is not None
    )
    proposals: list[tuple[GussetRib, tuple[FaceLike, ...], SolidRef]] = []
    links: list[list[int]] = [[] for _ in caps]
    for index, left in enumerate(caps):
        for other_index in range(index + 1, len(caps)):
            right = caps[other_index]
            if left.axis != right.axis or left.slant != right.slant:
                continue
            tol = length_tol(abs(right.at - left.at), rel=1e-7)
            if abs(right.at - left.at) <= tol or left.supports != right.supports:
                continue
            matching_caps = all(
                abs(a - b) <= tol
                for point_a, point_b in zip(left.profile, right.profile, strict=True)
                for a, b in zip(point_a, point_b, strict=True)
            )
            rounded_edge = left.transition != left.slant or right.transition != right.slant
            if matching_caps or rounded_edge:
                links[index].append(other_index)
                links[other_index].append(index)
    for index, left in enumerate(caps):
        if len(links[index]) != 1:
            continue
        other_index = links[index][0]
        if other_index < index or len(links[other_index]) != 1:
            continue
        right = caps[other_index]
        defining = tuple(
            dict.fromkeys((left.face, right.face, left.slant, left.transition, right.transition))
        )
        nodes = tuple(graph.require_node(face) for face in defining)
        support_nodes = tuple(graph.require_node(face) for face in left.supports)
        solid = graph.common_valid_solid((*nodes, *support_nodes))
        if solid is None:
            continue
        low, high = sorted((left, right), key=lambda cap: cap.at)
        thickness = high.at - low.at
        # One-sided fillets leave unequal end caps. Their smaller triangle must still be
        # filled all the way to the opposite cap; the removed blend lies outside that probe.
        probe_cap = min((low, high), key=lambda cap: cap.face.area)
        sign = 1 if probe_cap is low else -1
        direction = tuple(sign if i == left.axis else 0 for i in range(3))
        prism = extrude(probe_cap.face, amount=thickness, dir=direction)
        residual = prism - graph.solid_shape(solid)
        volume_tol = prism.volume / thickness * length_tol(thickness, rel=1e-7)
        if residual.volume > volume_tol:
            continue
        axes = tuple(i for i in range(3) if i != left.axis)
        bounds = left.slant.bounding_box()
        slant_span = (
            (bounds.min.X, bounds.max.X),
            (bounds.min.Y, bounds.max.Y),
            (bounds.min.Z, bounds.max.Z),
        )
        legs = tuple(slant_span[axis][1] - slant_span[axis][0] for axis in axes)
        tol = length_tol(min(legs), rel=1e-7)
        if any(leg <= tol for leg in legs):
            continue
        if any(
            min(abs(coord - side) for side in slant_span[axis]) > tol
            for axis, coord in zip(axes, left.support_coords, strict=True)
        ):
            continue
        record = GussetRib(
            thickness_axis="xyz"[left.axis],
            thickness_bounds=(round(low.at, 3), round(high.at, 3)),
            supports=(
                ("xyz"[axes[0]], round(left.support_coords[0], 3)),
                ("xyz"[axes[1]], round(left.support_coords[1], 3)),
            ),
            legs=(round(legs[0], 3), round(legs[1], 3)),
            directions=(
                1 if abs(left.support_coords[0] - slant_span[axes[0]][0]) <= tol else -1,
                1 if abs(left.support_coords[1] - slant_span[axes[1]][0]) <= tol else -1,
            ),
        )
        proposals.append((record, defining, solid))
    if proposals:
        solids = tuple(
            dict.fromkeys(
                solid
                for node in graph.nodes
                if (solid := graph.common_valid_solid((node,))) is not None
            )
        )
        keys = unambiguous_body_keys(
            tuple(graph.solid_shape(solid) for solid in solids),
            require_valid_solid=True,
            properties=graph.solid_properties,
        )
        by_solid = dict(zip(solids, keys, strict=True))
        proposals = [
            (replace(record, body_key=by_solid[solid]), defining, solid)
            for record, defining, solid in proposals
        ]
    proposals.sort(key=lambda item: item[0])
    if sink is not None:
        for record, defining, _ in proposals:
            sink.propose(
                FamilyId.GUSSET_RIBS,
                record,
                defining=tuple(graph.require_node(face) for face in defining),
            )
    return [record for record, _, _ in proposals]


def recognise_gusset_ribs(
    part: Part,
    *,
    face_edges: FaceEdges | None = None,
    ledger: ClaimLedger | EvidenceWriter | None = None,
) -> list[GussetRib]:
    """Return right-triangular material ribs bridging two principal planes."""
    graph = FaceGraph(part, face_edges=face_edges) if ledger is None else ledger.graph
    sink = None if ledger is None else ledger.sink
    return _discover_gusset_ribs(part, graph=graph, face_edges=face_edges, sink=sink)


def recognise_gusset_rib_patterns(
    ribs: Sequence[GussetRib],
) -> list[GussetRibArray | GussetRibMirrorPair]:
    """Group accepted, same-body ribs into constant-pitch arrays or centred mirror pairs."""

    groups: dict[tuple[object, ...], list[GussetRib]] = {}
    for rib in ribs:
        if not rib.body_key:
            continue
        width = round(rib.thickness_bounds[1] - rib.thickness_bounds[0], 3)
        spec = (
            rib.body_key,
            rib.thickness_axis,
            rib.supports,
            rib.legs,
            rib.directions,
            width,
        )
        groups.setdefault(spec, []).append(rib)

    patterns: list[GussetRibArray | GussetRibMirrorPair] = []
    for members in groups.values():
        members.sort(key=lambda rib: rib.thickness_bounds)
        if len(members) < 2:
            continue
        body_key = members[0].body_key
        assert body_key is not None
        axis = members[0].thickness_axis
        width = members[0].thickness_bounds[1] - members[0].thickness_bounds[0]
        axis_index = "xyz".index(axis)
        body_mid = (body_key[axis_index] + body_key[axis_index + 3]) / 2
        centres = [sum(rib.thickness_bounds) / 2 for rib in members]
        used: set[int] = set()

        start = 0
        while start + 2 < len(members):
            pitch = centres[start + 1] - centres[start]
            tol = _pattern_tol(pitch) if pitch > 0 else 0
            if pitch <= width + tol:
                start += 1
                continue
            end = start + 2
            while end < len(members) and abs(centres[end] - centres[end - 1] - pitch) <= tol:
                end += 1
            if end - start >= 3:
                indexes = range(start, end)
                patterns.append(
                    GussetRibArray(
                        tuple(members[index] for index in indexes),
                        axis,
                        round((centres[end - 1] - centres[start]) / (end - start - 1), 3),
                    )
                )
                used.update(indexes)
                start = end
            else:
                start += 1

        for index, low in enumerate(members):
            if index in used or centres[index] >= body_mid:
                continue
            tol = _pattern_tol(max(abs(centres[index] - body_mid), width))
            matches = [
                other
                for other in range(index + 1, len(members))
                if other not in used
                and centres[other] > body_mid
                and abs(centres[index] + centres[other] - 2 * body_mid) <= tol
                and members[other].thickness_bounds[0] - low.thickness_bounds[1] > tol
            ]
            if len(matches) == 1:
                other = matches[0]
                patterns.append(
                    GussetRibMirrorPair((low, members[other]), (axis, round(body_mid, 3)))
                )
                used.update((index, other))
    return sorted(patterns, key=lambda pattern: pattern.ribs[0].thickness_bounds)


# What this family declares about itself; `_registry` decides where it runs.
DEFINITION = PhysicalDefinition(
    FamilyId.GUSSET_RIBS,
    (GussetRib,),
    "gusset_ribs",
    "recognise_gusset_ribs",
    (),
    prismatic,
    simple(
        lambda s: list(
            _discover_gusset_ribs(
                s.context.part,
                graph=s.context.graph,
                face_edges=s.context.face_edges,
                sink=s.writer.sink,
            )
        )
    ),
    Counted("gusset_rib"),
    FullyAttributed("every returned gusset rib claims both end caps, its slant and edge blends"),
    evidence=Evidence(
        goldens=("gusset_ribs",), tests=("tests/test_gussets.py",), introduced="0.2.10"
    ),
)


def _derive_patterns(inputs: AcceptedInputs) -> list[object]:
    return list(recognise_gusset_rib_patterns(inputs.records(FamilyId.GUSSET_RIBS, GussetRib)))


PATTERNS = DerivedDefinition(
    DerivedId.GUSSET_RIB_PATTERNS,
    (GussetRibArray, GussetRibMirrorPair),
    "gusset_rib_patterns",
    "recognise_gusset_rib_patterns",
    (FamilyId.GUSSET_RIBS,),
    _derive_patterns,
    NotCounted("a relation among already counted gusset ribs"),
    evidence=Evidence(
        goldens=("gusset_rib_patterns",), tests=("tests/test_gussets.py",), introduced="0.2.10"
    ),
)
