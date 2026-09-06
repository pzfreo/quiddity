# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Original-face proof for a polygonal passage through a convex two-plane roof."""

from __future__ import annotations

from dataclasses import dataclass
from itertools import combinations
from typing import cast

from build123d import Compound, Face, GeomType, Keep, Plane, Shape, ShapeList, Solid, Vector, Wire

from quiddity._adjacency import FaceGraph, FaceNode, SolidRef, connected_components
from quiddity._section_passages import _ordered_cycle, _pair_line
from quiddity._sections import LocalFrame, PlanarSection, SectionVertex
from quiddity._support_patches import covered_patch
from quiddity._volume_probe import material_fraction

Vector3 = tuple[float, float, float]
PlaneTerm = tuple[float, tuple[float, float]]


@dataclass(frozen=True, slots=True)
class PlaneEnvelopePassageProof:
    walls: tuple[FaceNode, ...]
    planar_context: FaceNode
    roof_contexts: tuple[FaceNode, FaceNode]
    owner: SolidRef
    frame: LocalFrame
    section: PlanarSection
    run_interval: tuple[float, float]
    envelope_end: int
    terms: tuple[PlaneTerm, PlaneTerm]
    volume: float


def _shape(value: Shape | ShapeList | list[Solid] | None) -> Shape:
    if value is None:
        return Compound([])
    return Compound(value) if isinstance(value, (list, ShapeList)) else value


def _prove(
    graph: FaceGraph, mouth_node: FaceNode, walls: tuple[FaceNode, ...], base: LocalFrame
) -> PlaneEnvelopePassageProof | None:
    if len(walls) < 3:
        return None
    run = Vector(*base.run)
    normal = Vector(*cast(Vector3, graph.normal(mouth_node)))
    index = 1 if normal.dot(run) < 0 else 0
    sign = 1 if index == 1 else -1
    contexts = set().union(*(set(graph.neighbours(w)) for w in walls)) - set(walls) - {mouth_node}
    if len(contexts) != 2:
        return None
    roofs = tuple(sorted(contexts, key=lambda n: n.index))
    if any(
        not graph.is_planar(n)
        or (roof_normal := graph.normal(n)) is None
        or Vector(*roof_normal).dot(run) * sign <= 1e-8
        for n in roofs
    ):
        return None
    if not (graph.arc(*roofs) == "convex" and graph.shared_edges(*roofs)):
        return None
    if any(not any(graph.arc(w, r) == "convex" for r in roofs) for w in walls):
        return None
    owner = graph.common_valid_solid((*walls, mouth_node, *roofs))
    if owner is None:
        return None
    adjacency = {a: {b for b in walls if b != a and graph.arc(a, b) == "concave"} for a in walls}
    if any(len(neighbours) != 2 for neighbours in adjacency.values()):
        return None
    lines = {}
    for a, b in combinations(walls, 2):
        if b in adjacency[a]:
            line = _pair_line(graph, a, b, base)
            if line is None:
                return None
            lines[frozenset((a, b))] = line
    order = _ordered_cycle(walls, adjacency, lines)
    if len(set(order)) != len(walls):
        return None
    raw = PlanarSection(
        tuple(
            SectionVertex(lines[frozenset((w, order[(i + 1) % len(order)]))][:2])
            for i, w in enumerate(order)
        )
    )
    points = tuple(v.point for v in raw.boundary)
    for i, p in enumerate(points):
        previous, following = points[i - 1], points[(i + 1) % len(points)]
        if (p[0] - previous[0]) * (following[1] - p[1]) - (p[1] - previous[1]) * (
            following[0] - p[0]
        ) < 0:
            return None

    def world(point, height):
        return Vector(*base.u) * point[0] + Vector(*base.v) * point[1] + run * height

    far = graph.face(mouth_node).center().dot(run)
    heights = [Vector(*v).dot(run) for w in walls for v in graph.face(w).vertices()]
    span = max(abs(h - far) for h in heights)
    if span <= 1e-6:
        return None
    bound = (max(heights) if index == 1 else min(heights)) + sign * max(1e-3, span * 0.01)
    mouth = Face(Wire.make_polygon([world(p, far) for p in points], close=True))
    cell = Solid.extrude(mouth, run * (bound - far))
    for roof in roofs:
        cell = _shape(
            cell.split(
                Plane(origin=graph.face(roof).center(), z_dir=cast(Vector3, graph.normal(roof))),
                Keep.BOTTOM,
            )
        )
    if not cell.is_valid or len(cell.solids()) != 1 or cell.volume <= 1e-12:
        return None
    faces = tuple(cell.faces())
    if any(f.geom_type != GeomType.PLANE for f in faces):
        return None
    lateral = tuple(f for f in faces if abs(f.normal_at().dot(run)) < 1e-8)
    source_walls = tuple(graph.face(w) for w in walls)
    if any(not covered_patch(f, source_walls) for f in lateral) or any(
        not covered_patch(f, lateral) for f in source_walls
    ):
        return None
    opposite = tuple(f for f in faces if f.normal_at().dot(run) * sign < -1 + 1e-8)
    if not covered_patch(mouth, opposite) or any(not covered_patch(f, (mouth,)) for f in opposite):
        return None
    terminal = tuple(f for f in faces if f not in lateral and f not in opposite)
    matches: list[Face] = []
    for roof in roofs:
        n = Vector(*cast(Vector3, graph.normal(roof)))
        roof_point = graph.face(roof).center()
        selected = tuple(
            f
            for f in terminal
            if f.normal_at().dot(n) > 1 - 1e-8
            and all(abs((Vector(*v) - roof_point).dot(n)) < 1e-6 for v in f.vertices())
        )
        if not selected or sum(f.area for f in selected) <= 1e-10:
            return None
        matches.extend(selected)
    if len(matches) != len(terminal) or any(matches.count(f) != 1 for f in terminal):
        return None
    solid = graph.solid_shape(owner)
    if material_fraction(solid, cell) > 1e-9:
        return None
    for direction in (-run, run):
        probe = _shape(cell.translate(direction * max(2e-5, span * 1e-4)).cut(cell))
        if probe.volume <= 1e-12 or material_fraction(solid, probe) > 1e-9:
            return None
    origin = world(raw.centroid, 0)
    frame = LocalFrame.canonical(base.run, cast(Vector3, tuple(origin)))
    section = PlanarSection(
        tuple(SectionVertex((p[0] - raw.centroid[0], p[1] - raw.centroid[1])) for p in points)
    )
    terms = []
    for roof in roofs:
        n = Vector(*cast(Vector3, graph.normal(roof)))
        divisor = n.dot(run)
        terms.append(
            (
                (graph.face(roof).center() - origin).dot(n) / divisor,
                (-n.dot(Vector(*frame.u)) / divisor, -n.dot(Vector(*frame.v)) / divisor),
            )
        )
    select = min if index == 1 else max
    height = select(term[0] for term in terms)
    return PlaneEnvelopePassageProof(
        tuple(sorted(walls, key=lambda n: n.index)),
        mouth_node,
        cast(tuple[FaceNode, FaceNode], roofs),
        owner,
        frame,
        section,
        (far, height) if index == 1 else (height, far),
        index,
        cast(tuple[PlaneTerm, PlaneTerm], tuple(sorted(terms))),
        cell.volume,
    )


def plane_envelope_passage_proofs(graph: FaceGraph) -> tuple[PlaneEnvelopePassageProof, ...]:
    """Use original mouth/wall adjacency, without expected side counts or labels."""
    found = []
    for mouth in graph.nodes:
        if not graph.is_planar(mouth) or (normal := graph.normal(mouth)) is None:
            continue
        base = LocalFrame.canonical(normal, (0, 0, 0))
        run = Vector(*base.run)
        candidates = tuple(
            w
            for w in graph.neighbours(mouth)
            if graph.is_planar(w)
            and (n := graph.normal(w)) is not None
            and abs(Vector(*n).dot(run)) < 1e-8
            and graph.arc(mouth, w) == "convex"
        )
        for walls in connected_components(candidates, lambda a, b: graph.arc(a, b) == "concave"):
            try:
                proof = _prove(graph, mouth, walls, base)
            except (ValueError, RuntimeError, TypeError, ZeroDivisionError):
                continue
            if proof is not None:
                found.append(proof)
    return tuple(found)
