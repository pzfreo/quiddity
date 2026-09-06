# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Neutral constant-section planar-wall rings on an arbitrary run direction."""

from __future__ import annotations

import math
from collections import defaultdict
from dataclasses import dataclass
from typing import cast

from build123d import Face, Solid, Vector, Wire

from quiddity._adjacency import FaceGraph, FaceNode, SolidRef, connected_components
from quiddity._sections import (
    BodyRef,
    BodyRefIssuer,
    LocalFrame,
    PlanarSection,
    SectionEnds,
    SectionOccurrence,
    SectionVertex,
    validate_occurrence,
)
from quiddity._typing import Part
from quiddity._volume_probe import material_fraction as _material_fraction
from quiddity._wire_seed import wire_seed as _wire_seed

_DIRECTION_TOL = 2e-8
_INTERVAL_TOL = 1e-6
_END_PROBE = 2e-5
_COORD_FLOOR = 1e-6
_MATERIAL_VOL_FRAC = 1e-9

Vector3 = tuple[float, float, float]


@dataclass(frozen=True, slots=True)
class SectionRingProposal:
    occurrence: SectionOccurrence
    nodes: tuple[FaceNode, ...]
    solid: SolidRef
    body_adapter: _BodyAdapter
    constituent: frozenset[FaceNode] = frozenset()
    low_gradient: tuple[float, float] = (0.0, 0.0)
    high_gradient: tuple[float, float] = (0.0, 0.0)

    @property
    def frame(self) -> LocalFrame:
        return self.occurrence.frame

    @property
    def run_interval(self) -> tuple[float, float]:
        return self.occurrence.run_interval

    @property
    def section(self) -> PlanarSection:
        return self.occurrence.section

    @property
    def ends(self) -> SectionEnds:
        return self.occurrence.ends


class _BodyAdapter:
    """One-to-one bridge between graph and neutral section body authorities."""

    def __init__(self) -> None:
        self._issuer = BodyRefIssuer()
        self._pairs: dict[SolidRef, BodyRef] = {}

    def body(self, solid: SolidRef) -> BodyRef:
        current = self._pairs.get(solid)
        if current is None:
            current = self._issuer.issue()
            self._pairs[solid] = current
        return current

    def validate(self, solid: SolidRef, occurrence: SectionOccurrence) -> None:
        if self._pairs.get(solid) is not occurrence.body:
            raise ValueError("section occurrence body does not match its graph solid")
        validate_occurrence(occurrence, body_refs=self._issuer)


def _dot(left: Vector3, right: Vector3) -> float:
    return sum(a * b for a, b in zip(left, right, strict=True))


def _point(value: object) -> Vector3:
    return (float(value.X), float(value.Y), float(value.Z))  # type: ignore[attr-defined]


def _canonical_run(edge: object) -> Vector3 | None:
    try:
        if edge.geom_type.name != "LINE":  # type: ignore[attr-defined]
            return None
        tangent = edge.tangent_at().normalized()  # type: ignore[attr-defined]
        frame = LocalFrame.canonical(_point(tangent), (0.0, 0.0, 0.0))
    except (AttributeError, TypeError, ValueError, ZeroDivisionError):
        return None
    return frame.run


def _parallel(left: Vector3, right: Vector3) -> bool:
    return abs(abs(_dot(left, right)) - 1.0) <= _DIRECTION_TOL


def _parallel_pair_candidates(
    candidates: tuple[tuple[FaceNode, FaceNode, Vector3], ...], run: Vector3
) -> tuple[tuple[FaceNode, FaceNode, Vector3], ...]:
    """Retain the semantic tolerance across presentation-only direction buckets."""

    return tuple(candidate for candidate in candidates if _parallel(candidate[2], run))


def _pair_line(
    graph: FaceGraph, left: FaceNode, right: FaceNode, frame: LocalFrame
) -> tuple[float, float, float, float] | None:
    """Return one collinear junction as ``(u, v, t_low, t_high)``."""

    samples: list[tuple[float, float, float]] = []
    segments: list[tuple[float, float]] = []
    for edge in graph.shared_edges(left, right):
        run = _canonical_run(edge)
        if run is None or not _parallel(run, frame.run):
            return None
        try:
            endpoints = (_point(edge.position_at(0.0)), _point(edge.position_at(1.0)))
        except (AttributeError, RuntimeError, TypeError, ValueError):
            return None
        projected = tuple(
            (_dot(point, frame.u), _dot(point, frame.v), _dot(point, frame.run))
            for point in endpoints
        )
        samples.extend(projected)
        segments.append(tuple(sorted((projected[0][2], projected[1][2]))))  # type: ignore[arg-type]
    if not samples:
        return None
    u = sum(item[0] for item in samples) / len(samples)
    v = sum(item[1] for item in samples) / len(samples)
    if any(math.hypot(item[0] - u, item[1] - v) > _INTERVAL_TOL for item in samples):
        return None
    ordered = sorted(segments)
    for previous, following in zip(ordered, ordered[1:], strict=False):
        delta = following[0] - previous[1]
        if delta > _INTERVAL_TOL or delta < -_INTERVAL_TOL:
            return None
    return u, v, ordered[0][0], ordered[-1][1]


def _face_interval(graph: FaceGraph, node: FaceNode, run: Vector3) -> tuple[float, float] | None:
    try:
        values = tuple(_dot(_point(vertex), run) for vertex in graph.face(node).vertices())
    except (AttributeError, RuntimeError, TypeError, ValueError):
        return None
    return (min(values), max(values)) if values else None


def _bounded_inner_region(
    graph: FaceGraph, opening: FaceNode, seed: frozenset[FaceNode]
) -> frozenset[FaceNode]:
    region = set(seed)
    pending = list(seed)
    while pending:
        current = pending.pop()
        for neighbour in graph.neighbours(current):
            if neighbour is opening or neighbour in region:
                continue
            kind = graph.arc(current, neighbour)
            if kind not in ("concave", "smooth"):
                continue
            region.add(neighbour)
            pending.append(neighbour)
    return frozenset(region)


def _mouth_regions(
    graph: FaceGraph,
) -> tuple[tuple[frozenset[FaceNode], tuple[tuple[FaceNode, Wire, frozenset[FaceNode]], ...]], ...]:
    by_region: dict[frozenset[FaceNode], list[tuple[FaceNode, Wire, frozenset[FaceNode]]]] = (
        defaultdict(list)
    )
    for opening in graph.nodes:
        if not graph.is_planar(opening):
            continue
        for wire in graph.face(opening).inner_wires():
            seed = _wire_seed(graph, opening, wire)
            if (
                len(seed) < 3
                or any(not graph.is_planar(node) for node in seed)
                or not all(graph.arc(opening, node) == "convex" for node in seed)
            ):
                continue
            by_region[_bounded_inner_region(graph, opening, seed)].append((opening, wire, seed))
    return tuple(
        (region, tuple(sorted(mouths, key=lambda item: item[0].index)))
        for region, mouths in sorted(
            by_region.items(),
            key=lambda item: tuple(sorted(node.index for node in item[0])),
        )
    )


def _line_section(wire: Wire, base: LocalFrame) -> tuple[PlanarSection, Vector3] | None:
    try:
        edges = tuple(wire.edges())
        if any(edge.geom_type.name != "LINE" for edge in edges):
            return None
        points = []
        for index, edge in enumerate(edges):
            shared = tuple(
                left
                for left in edges[index - 1].vertices()
                for right in edge.vertices()
                if left == right
            )
            if len(shared) != 1:
                return None
            points.append(_point(shared[0]))
        ordered_points = tuple(points)
        if len(ordered_points) < 3:
            return None
        raw = PlanarSection(
            tuple(
                SectionVertex((_dot(point, base.u), _dot(point, base.v)))
                for point in ordered_points
            )
        )
        centre = raw.centroid
        world_centre = tuple(
            centre[0] * base.u[index] + centre[1] * base.v[index] for index in range(3)
        )
        section = PlanarSection(
            tuple(
                SectionVertex((vertex.point[0] - centre[0], vertex.point[1] - centre[1]))
                for vertex in raw.boundary
            )
        )
    except (AttributeError, RuntimeError, TypeError, ValueError):
        return None
    return section, cast(Vector3, world_centre)


def _same_section(left: PlanarSection, right: PlanarSection) -> bool:
    return len(left.boundary) == len(right.boundary) and all(
        math.dist(a.point, b.point) <= _INTERVAL_TOL
        for a, b in zip(left.boundary, right.boundary, strict=True)
    )


def _wall_run(graph: FaceGraph, region: frozenset[FaceNode]) -> Vector3 | None:
    """Return the unique straight junction direction proved by one planar wall region."""

    if len(region) < 3 or any(not graph.is_planar(node) for node in region):
        return None
    runs: list[Vector3] = []
    adjacency: dict[FaceNode, set[FaceNode]] = defaultdict(set)
    for left in region:
        for right in graph.neighbours(left):
            if right not in region or right.index <= left.index:
                continue
            edges = tuple(graph.shared_edges(left, right))
            edge_runs = tuple(_canonical_run(edge) for edge in edges)
            if not edge_runs or any(run is None for run in edge_runs):
                return None
            runs.extend(cast(tuple[Vector3, ...], edge_runs))
            adjacency[left].add(right)
            adjacency[right].add(left)
    if not runs or any(len(adjacency[node]) != 2 for node in region):
        return None
    run = min(runs)
    if any(not _parallel(candidate, run) for candidate in runs):
        return None
    if any(
        (normal := graph.normal(node)) is None or abs(_dot(normal, run)) > _DIRECTION_TOL
        for node in region
    ):
        return None
    return run


def _termination_plane(
    normal: Vector3, wire: Wire, frame: LocalFrame
) -> tuple[float, tuple[float, float]] | None:
    """Express one planar mouth as ``t = at + du*x + dv*y`` in *frame*."""

    along = _dot(normal, frame.run)
    if abs(along) <= _DIRECTION_TOL:
        return None
    try:
        point = _point(wire.vertices()[0])
    except (AttributeError, IndexError, RuntimeError, TypeError, ValueError):
        return None
    delta = cast(Vector3, tuple(point[i] - frame.origin[i] for i in range(3)))
    at = _dot(normal, delta) / along
    gradient = (-_dot(normal, frame.u) / along, -_dot(normal, frame.v) / along)
    if not all(math.isfinite(value) for value in (at, *gradient)):
        return None
    return at, gradient


def _plane_wire(
    frame: LocalFrame,
    at: float,
    gradient: tuple[float, float],
    section: PlanarSection,
) -> Wire:
    points = tuple(
        Vector(
            *_world(
                frame,
                at + gradient[0] * vertex.point[0] + gradient[1] * vertex.point[1],
                vertex.point,
            )
        )
        for vertex in section.boundary
    )
    return Wire.make_polygon((*points, points[0]))


def _between_planes(
    frame: LocalFrame,
    low: tuple[float, tuple[float, float]],
    high: tuple[float, tuple[float, float]],
    section: PlanarSection,
) -> Solid:
    if any(
        low[0] + low[1][0] * vertex.point[0] + low[1][1] * vertex.point[1]
        >= high[0] + high[1][0] * vertex.point[0] + high[1][1] * vertex.point[1]
        for vertex in section.boundary
    ):
        raise ValueError("passage termination planes cross")
    return Solid.make_loft(
        [
            _plane_wire(frame, low[0], low[1], section),
            _plane_wire(frame, high[0], high[1], section),
        ],
        ruled=True,
    )


def _void_and_planar_open(
    solid: Part,
    frame: LocalFrame,
    low: tuple[float, tuple[float, float]],
    high: tuple[float, tuple[float, float]],
    section: PlanarSection,
) -> bool:
    """Prove an empty clipped prism and exterior void beyond both planar mouths."""

    try:
        scale = max(1.0, high[0] - low[0])
        radius = max(math.hypot(*vertex.point) for vertex in section.boundary)
        thickness = max(_END_PROBE, scale * 1e-4, radius * 1e-4)
        inner = _between_planes(
            frame,
            (low[0] + _COORD_FLOOR, low[1]),
            (high[0] - _COORD_FLOOR, high[1]),
            section,
        )
        low_slab = _between_planes(
            frame,
            (low[0] - thickness, low[1]),
            (low[0] - _COORD_FLOOR, low[1]),
            section,
        )
        high_slab = _between_planes(
            frame,
            (high[0] + _COORD_FLOOR, high[1]),
            (high[0] + thickness, high[1]),
            section,
        )
        return _material_fraction(solid, inner) <= _MATERIAL_VOL_FRAC and all(
            _material_fraction(solid, slab) <= _MATERIAL_VOL_FRAC for slab in (low_slab, high_slab)
        )
    except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return False


def _enclosure_proposals(graph: FaceGraph, bodies: _BodyAdapter) -> tuple[SectionRingProposal, ...]:
    proposals = []
    for region, mouths in _mouth_regions(graph):
        if len(mouths) != 2:
            continue
        (first_opening, first_wire, first_seed), (second_opening, second_wire, second_seed) = mouths
        first_normal = graph.normal(first_opening)
        second_normal = graph.normal(second_opening)
        solid = graph.common_valid_solid(region | {first_opening, second_opening})
        if first_normal is None or second_normal is None or solid is None:
            continue
        parallel_mouths = _parallel(first_normal, second_normal)
        if parallel_mouths and _dot(first_normal, second_normal) > 0.0:
            continue
        run = _wall_run(graph, region)
        # Parallel stock faces need not be perpendicular to the passage. Their
        # normals describe termination planes, not the independently proved run.
        if not parallel_mouths or (run is not None and not _parallel(run, first_normal)):
            if run is None:
                continue
            base = LocalFrame.canonical(run, (0.0, 0.0, 0.0))
            first = _line_section(first_wire, base)
            second = _line_section(second_wire, base)
            if (
                first is None
                or second is None
                or not _same_section(first[0], second[0])
                or math.dist(first[1], second[1]) > _INTERVAL_TOL
            ):
                continue
            section, centre = first
            frame = LocalFrame.canonical(base.run, centre)
            first_plane = _termination_plane(first_normal, first_wire, frame)
            second_plane = _termination_plane(second_normal, second_wire, frame)
            if first_plane is None or second_plane is None:
                continue
            low, high = sorted((first_plane, second_plane), key=lambda item: item[0])
            if high[0] - low[0] <= _COORD_FLOOR or not _void_and_planar_open(
                graph.solid_shape(solid), frame, low, high, section
            ):
                continue
            defining = tuple(sorted(first_seed | second_seed, key=lambda node: node.index))
            occurrence = SectionOccurrence(
                bodies.body(solid),
                frame,
                (low[0], high[0]),
                section,
                SectionEnds(False, False),
            )
            bodies.validate(solid, occurrence)
            proposals.append(
                SectionRingProposal(
                    occurrence,
                    defining,
                    solid,
                    bodies,
                    constituent=region,
                    low_gradient=low[1],
                    high_gradient=high[1],
                )
            )
            continue
        base = LocalFrame.canonical(first_normal, (0.0, 0.0, 0.0))
        first = _line_section(first_wire, base)
        second = _line_section(second_wire, base)
        if first is None or second is None or not _same_section(first[0], second[0]):
            continue
        section, centre = first
        frame = LocalFrame.canonical(base.run, centre)
        interval = tuple(
            sorted(
                (
                    _dot(_point(first_wire.vertices()[0]), frame.run),
                    _dot(_point(second_wire.vertices()[0]), frame.run),
                )
            )
        )
        if interval[1] - interval[0] <= _COORD_FLOOR or not _void_and_open(
            graph.solid_shape(solid), frame, cast(tuple[float, float], interval), section
        ):
            continue
        defining = tuple(sorted(first_seed | second_seed, key=lambda node: node.index))
        occurrence = SectionOccurrence(
            bodies.body(solid),
            frame,
            cast(tuple[float, float], interval),
            section,
            SectionEnds(False, False),
        )
        bodies.validate(solid, occurrence)
        proposals.append(
            SectionRingProposal(occurrence, defining, solid, bodies, constituent=region)
        )
    return tuple(proposals)


def _world(frame: LocalFrame, t: float, point: tuple[float, float]) -> Vector3:
    return tuple(
        frame.origin[index]
        + t * frame.run[index]
        + point[0] * frame.u[index]
        + point[1] * frame.v[index]
        for index in range(3)
    )  # type: ignore[return-value]


def _probe_prism(
    frame: LocalFrame,
    interval: tuple[float, float],
    section: PlanarSection,
) -> Solid:
    low, high = interval
    if high - low <= 2 * _COORD_FLOOR:
        raise ValueError("section prism is too short to classify")
    points = tuple(
        Vector(*_world(frame, low + _COORD_FLOOR, vertex.point)) for vertex in section.boundary
    )
    wire = Wire.make_polygon((*points, points[0]))
    vector = Vector(*(component * (high - low - 2 * _COORD_FLOOR) for component in frame.run))
    return Solid.extrude(Face(wire), vector)


def _end_slab(
    frame: LocalFrame,
    end: float,
    sign: float,
    thickness: float,
    section: PlanarSection,
) -> Solid:
    """Build the complete section slab strictly outside one occurrence end."""

    inner = end + sign * _COORD_FLOOR
    outer = end + sign * thickness
    low, high = sorted((inner, outer))
    if high - low <= _COORD_FLOOR:
        raise ValueError("section end slab is too thin to classify")
    points = tuple(Vector(*_world(frame, low, vertex.point)) for vertex in section.boundary)
    wire = Wire.make_polygon((*points, points[0]))
    return Solid.extrude(
        Face(wire),
        Vector(*(component * (high - low) for component in frame.run)),
    )


def _void_and_open(
    solid: Part,
    frame: LocalFrame,
    interval: tuple[float, float],
    section: PlanarSection,
) -> bool:
    try:
        if _material_fraction(solid, _probe_prism(frame, interval, section)) > _MATERIAL_VOL_FRAC:
            return False
        scale = max(1.0, interval[1] - interval[0])
        radius = max(math.hypot(*vertex.point) for vertex in section.boundary)
        thickness = max(_END_PROBE, scale * 1e-4, radius * 1e-4)
        return all(
            _material_fraction(solid, _end_slab(frame, end, sign, thickness, section))
            <= _MATERIAL_VOL_FRAC
            for end, sign in ((interval[0], -1.0), (interval[1], 1.0))
        )
    except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return False


def _ordered_cycle(
    members: tuple[FaceNode, ...],
    adjacency: dict[FaceNode, set[FaceNode]],
    pair_lines: dict[frozenset[FaceNode], tuple[float, float, float, float]],
) -> tuple[FaceNode, ...]:
    """Choose the cycle by its geometric corner sequence, never node/traversal order."""

    candidates: list[tuple[tuple[tuple[float, float], ...], tuple[FaceNode, ...]]] = []
    for start in members:
        for first in adjacency[start]:
            order = [start, first]
            while len(order) < len(members):
                choices = adjacency[order[-1]] - {order[-2]}
                if len(choices) != 1:
                    raise ValueError("section wall component is not one simple cycle")
                order.append(next(iter(choices)))
            closed = tuple(order)
            corners = tuple(
                pair_lines[frozenset((node, closed[(at + 1) % len(closed)]))][:2]
                for at, node in enumerate(closed)
            )
            candidates.append((corners, closed))
    return min(candidates, key=lambda item: item[0])[1]


def section_ring_proposals(part: Part, graph: FaceGraph) -> tuple[SectionRingProposal, ...]:
    """Return every supported line-walled, constant-section, two-open-end void."""

    bodies = _BodyAdapter()
    for face in part.faces():
        graph.require_node(face)
    planar = tuple(node for node in graph.nodes if graph.is_planar(node))
    direction_pairs: dict[tuple[float, float, float], list[tuple[FaceNode, FaceNode, Vector3]]] = (
        defaultdict(list)
    )
    inspected_pairs: set[frozenset[FaceNode]] = set()
    for left in planar:
        for right in graph.neighbours(left):
            pair = frozenset((left, right))
            if right not in planar or pair in inspected_pairs:
                continue
            inspected_pairs.add(pair)
            for edge in graph.shared_edges(left, right):
                run = _canonical_run(edge)
                if run is not None:
                    key = cast(Vector3, tuple(round(value, 9) for value in run))
                    direction_pairs[key].append((left, right, run))

    proposals: list[SectionRingProposal] = []
    seen: set[frozenset[FaceNode]] = set()
    discovered_pairs = tuple(
        candidate for candidates in direction_pairs.values() for candidate in candidates
    )
    for key in sorted(direction_pairs):
        run = min(item[2] for item in direction_pairs[key])
        base = LocalFrame.canonical(run, (0.0, 0.0, 0.0))
        candidates = _parallel_pair_candidates(discovered_pairs, base.run)
        candidate_nodes = {node for left, right, _run in candidates for node in (left, right)}
        walls = frozenset(
            node
            for node in candidate_nodes
            if (normal := graph.normal(node)) is not None
            and abs(_dot(normal, base.run)) <= _DIRECTION_TOL
        )
        pair_lines: dict[frozenset[FaceNode], tuple[float, float, float, float]] = {}
        adjacency: dict[FaceNode, set[FaceNode]] = defaultdict(set)
        candidate_pairs = {
            frozenset((left, right)): (left, right) for left, right, _run in candidates
        }
        for left, right in candidate_pairs.values():
            if left not in walls or right not in walls:
                continue
            line = _pair_line(graph, left, right, base)
            if line is None:
                continue
            left_span = _face_interval(graph, left, base.run)
            right_span = _face_interval(graph, right, base.run)
            if (
                left_span is None
                or right_span is None
                or any(
                    abs(actual - expected) > _INTERVAL_TOL
                    for actual, expected in zip(
                        (*left_span, *right_span),
                        (line[2], line[3], line[2], line[3]),
                        strict=True,
                    )
                )
            ):
                continue
            pair_lines[frozenset((left, right))] = line
            adjacency[left].add(right)
            adjacency[right].add(left)

        def connected(
            left: FaceNode,
            right: FaceNode,
            adjacency: dict[FaceNode, set[FaceNode]] = adjacency,
        ) -> bool:
            return right in adjacency[left]

        for component in connected_components(walls, connected):
            members = set(component)
            if len(component) < 3 or any(len(adjacency[node] & members) != 2 for node in component):
                continue
            identity = frozenset(component)
            solid = graph.common_valid_solid(component)
            if identity in seen or solid is None:
                continue
            order = _ordered_cycle(component, adjacency, pair_lines)
            lines = tuple(
                pair_lines[frozenset((node, order[(at + 1) % len(order)]))]
                for at, node in enumerate(order)
            )
            low, high = lines[0][2], lines[0][3]
            spans = tuple(_face_interval(graph, node, base.run) for node in order)
            if any(span is None for span in spans):
                continue
            complete_spans = cast(tuple[tuple[float, float], ...], spans)
            low, high = complete_spans[0]
            if any(
                abs(span[0] - low) > _INTERVAL_TOL or abs(span[1] - high) > _INTERVAL_TOL
                for span in complete_spans
            ):
                continue
            try:
                raw = PlanarSection(tuple(SectionVertex((line[0], line[1])) for line in lines))
                centre = raw.centroid
                frame = LocalFrame.canonical(
                    base.run,
                    tuple(
                        centre[0] * base.u[index] + centre[1] * base.v[index] for index in range(3)
                    ),  # type: ignore[arg-type]
                )
                section = PlanarSection(
                    tuple(
                        SectionVertex(
                            (vertex.point[0] - centre[0], vertex.point[1] - centre[1]),
                            vertex.bulge,
                        )
                        for vertex in raw.boundary
                    )
                )
            except ValueError:
                continue
            if not _void_and_open(graph.solid_shape(solid), frame, (low, high), section):
                continue
            seen.add(identity)
            occurrence = SectionOccurrence(
                bodies.body(solid),
                frame,
                (low, high),
                section,
                SectionEnds(False, False),
            )
            bodies.validate(solid, occurrence)
            proposals.append(
                SectionRingProposal(
                    occurrence,
                    order,
                    solid,
                    bodies,
                )
            )
    for proposal in _enclosure_proposals(graph, bodies):
        identity = frozenset(proposal.nodes)
        if identity in seen:
            continue
        seen.add(identity)
        proposals.append(proposal)
    proposals.sort(key=lambda item: (item.frame.run, item.run_interval, item.frame.origin))
    return tuple(proposals)
