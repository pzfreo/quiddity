# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Complete rolling-ball blend-path recognition.

A :class:`Blend` carries a discriminated straight or circular rolling path, one radius and one
proved material side. Convex chains describe external rounds; concave chains describe internal
rounds and may coexist with the Pocket, Slot or Step whose interior contains them. The narrower
:class:`~quiddity.fillets.Fillet` family remains the dimension-worthy external edge
treatment; aggregate reconciliation prefers that family when it describes a complete chain.

The recogniser consumes the immutable :class:`._blend_view.BlendCollapseIndex`.  It never copies
Analysis Situs rules, consults a corpus label, or infers membership after recognition.  Every
original rolling-surface patch in the chain is defining evidence.
"""

from __future__ import annotations

import math
from collections import deque
from dataclasses import dataclass

from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepGProp import BRepGProp
from OCP.GeomAbs import GeomAbs_Torus
from OCP.gp import gp_Pnt, gp_Vec
from OCP.GProp import GProp_GProps
from OCP.TopAbs import TopAbs_Orientation

from quiddity._adjacency import EdgeOccurrenceRef, FaceGraph, FaceNode
from quiddity._analytic_surfaces import equivalent_parameters
from quiddity._blend_view import BlendChain, BlendCollapseIndex
from quiddity._candidates import FamilyId
from quiddity._claims import EvidenceWriter
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact,
    EffectiveSurfaceIndex,
    SurfaceKind,
)
from quiddity._geometry import (
    SMOOTH_ARC_GAP,
    _canonical_axis_direction,
    _coaxial_axis_lines,
    length_tol,
)
from quiddity._record import Record
from quiddity._typing import Part


def _canonical_direction(direction: tuple[float, float, float]) -> tuple[float, float, float]:
    axis_at = max(range(3), key=lambda index: abs(direction[index]))
    return _canonical_axis_direction("xyz"[axis_at], direction)


@dataclass(frozen=True)
class StraightBlendPath(Record):
    """One analytic straight rolling path and a covariant point on that line."""

    at: tuple[float, float, float]
    direction: tuple[float, float, float]

    def __post_init__(self) -> None:
        if not all(math.isfinite(value) for value in self.at):
            raise ValueError("straight blend path anchor must be finite")
        object.__setattr__(self, "direction", _canonical_direction(self.direction))


@dataclass(frozen=True)
class CircularBlendPath(Record):
    """One complete circular rolling path."""

    center: tuple[float, float, float]
    normal: tuple[float, float, float]
    radius: float

    def __post_init__(self) -> None:
        if not all(math.isfinite(value) for value in self.center):
            raise ValueError("circular blend path centre must be finite")
        if not math.isfinite(self.radius) or self.radius <= 0.0:
            raise ValueError("circular blend path radius must be positive and finite")
        object.__setattr__(self, "normal", _canonical_direction(self.normal))


BlendPath = StraightBlendPath | CircularBlendPath


def _path_sort_key(path: BlendPath) -> tuple[object, ...]:
    if isinstance(path, StraightBlendPath):
        return ("straight", path.at, path.direction)
    return ("circular", path.center, path.normal, path.radius)


@dataclass(frozen=True)
class Blend(Record):
    """One complete rolling-ball occurrence.

    ``side`` is the proved material-side relation, ``"convex"`` for an external round or
    ``"concave"`` for an internal round. ``path`` explicitly distinguishes a straight cylinder
    axis from a complete circular torus centre-line.
    """

    radius: float
    side: str
    path: BlendPath

    def __post_init__(self) -> None:
        if not math.isfinite(self.radius) or self.radius <= 0.0:
            raise ValueError("blend radius must be positive and finite")
        if self.side not in ("convex", "concave"):
            raise ValueError("public blend side must be convex or concave")
        if not isinstance(self.path, (StraightBlendPath, CircularBlendPath)):
            raise TypeError("blend path must be a straight or circular Blend path")


@dataclass(frozen=True, slots=True)
class _BlendProposal:
    record: Blend
    nodes: tuple[FaceNode, ...]


@dataclass(frozen=True, slots=True)
class _NativeTorus:
    center: tuple[float, float, float]
    normal: tuple[float, float, float]
    major_radius: float
    minor_radius: float


def _native_torus(graph: FaceGraph, node: FaceNode) -> _NativeTorus | None:
    surface = BRepAdaptor_Surface(graph.face(node).wrapped)
    if surface.GetType() != GeomAbs_Torus:
        return None
    torus = surface.Torus()
    axis = torus.Axis()
    location = axis.Location()
    direction = axis.Direction()
    values = (
        float(location.X()),
        float(location.Y()),
        float(location.Z()),
        float(direction.X()),
        float(direction.Y()),
        float(direction.Z()),
        float(torus.MajorRadius()),
        float(torus.MinorRadius()),
    )
    if not all(map(math.isfinite, values)) or values[6] <= 0.0 or values[7] <= 0.0:
        return None
    # gp_Dir already supplies a unit axis. Keep its full precision for geometric proofs;
    # public direction canonicalization rounds components and can break tight alignment tests.
    # CircularBlendPath applies that output convention only after acceptance.
    return _NativeTorus(values[:3], values[3:6], values[6], values[7])


def _same_torus(left: _NativeTorus, right: _NativeTorus, *, local: float) -> bool:
    tolerance = length_tol(local, rel=1e-9)
    return (
        math.dist(left.center, right.center) <= tolerance
        and 1.0 - abs(math.fsum(a * b for a, b in zip(left.normal, right.normal, strict=True)))
        <= SMOOTH_ARC_GAP
        and abs(left.major_radius - right.major_radius) <= tolerance
        and abs(left.minor_radius - right.minor_radius) <= tolerance
    )


def _toroidal_components(
    graph: FaceGraph,
) -> tuple[tuple[frozenset[FaceNode], _NativeTorus], ...]:
    facts = {
        node: torus for node in graph.nodes if (torus := _native_torus(graph, node)) is not None
    }
    pending = set(facts)
    found: list[tuple[frozenset[FaceNode], _NativeTorus]] = []
    while pending:
        first = min(pending, key=lambda node: node.index)
        pending.remove(first)
        component = {first}
        queue = deque((first,))
        local = min(
            facts[first].major_radius,
            facts[first].minor_radius,
            math.sqrt(float(graph.face(first).area)),
        )
        while queue:
            current = queue.popleft()
            for neighbour in sorted(graph.neighbours(current), key=lambda node: node.index):
                if (
                    neighbour in pending
                    and graph.arc(current, neighbour) == "smooth"
                    and _same_torus(facts[first], facts[neighbour], local=local)
                ):
                    pending.remove(neighbour)
                    component.add(neighbour)
                    queue.append(neighbour)
        found.append((frozenset(component), facts[first]))
    return tuple(sorted(found, key=lambda item: min(node.index for node in item[0])))


def _support_region(
    graph: FaceGraph,
    surfaces: EffectiveSurfaceIndex,
    first: FaceNode,
    *,
    excluded: frozenset[FaceNode],
) -> frozenset[FaceNode]:
    initial = surfaces.fact(first)
    if not isinstance(initial, AnalyticSurfaceFact):
        return frozenset((first,))
    found = {first}
    queue = deque((first,))
    while queue:
        current = queue.popleft()
        for neighbour in sorted(graph.neighbours(current), key=lambda node: node.index):
            exact_smooth = graph.arc(current, neighbour) == "smooth"
            if (
                neighbour in found
                or neighbour in excluded
                or not exact_smooth
            ):
                continue
            fact = surfaces.fact(neighbour)
            local = min(
                math.sqrt(float(graph.face(first).area)),
                math.sqrt(float(graph.face(neighbour).area)),
            )
            if (
                isinstance(fact, AnalyticSurfaceFact)
                and fact.kind is initial.kind
                and equivalent_parameters(
                    initial.kind, initial.parameters, fact.parameters, local=local
                )
            ):
                found.add(neighbour)
                queue.append(neighbour)
    return frozenset(found)


def _covers_complete_circle(graph: FaceGraph, nodes: frozenset[FaceNode]) -> bool:
    intervals: list[tuple[float, float]] = []
    full = math.tau
    for node in nodes:
        surface = BRepAdaptor_Surface(graph.face(node).wrapped)
        start = float(surface.FirstUParameter())
        span = float(surface.LastUParameter() - start)
        if not math.isfinite(start) or not math.isfinite(span) or span <= 0.0:
            return False
        if span >= full - 1e-9:
            return True
        start %= full
        end = start + span
        if end <= full:
            intervals.append((start, end))
        else:
            intervals.extend(((start, full), (0.0, end - full)))
    covered = 0.0
    end = 0.0
    for start, stop in sorted(intervals):
        if start > end + 1e-9:
            return False
        if stop > end:
            covered += stop - max(start, end)
            end = stop
    return end >= full - 1e-9 and covered >= full - 1e-9


def _torus_side(graph: FaceGraph, node: FaceNode, torus: _NativeTorus) -> str | None:
    face = graph.face(node)
    surface = BRepAdaptor_Surface(face.wrapped)
    signs: set[int] = set()
    for u_fraction, v_fraction in ((0.25, 0.25), (0.5, 0.5), (0.75, 0.75)):
        u = float(surface.FirstUParameter()) + u_fraction * float(
            surface.LastUParameter() - surface.FirstUParameter()
        )
        v = float(surface.FirstVParameter()) + v_fraction * float(
            surface.LastVParameter() - surface.FirstVParameter()
        )
        point, along_u, along_v = gp_Pnt(), gp_Vec(), gp_Vec()
        try:
            surface.D1(u, v, point, along_u, along_v)
            normal = along_u.Crossed(along_v)
        except Exception:  # noqa: BLE001 - a degenerate patch has no material-side normal
            return None
        magnitude = float(normal.Magnitude())
        if not math.isfinite(magnitude) or magnitude <= 0.0:
            return None
        orientation = face.wrapped.Orientation()
        sign = -1.0 if orientation == TopAbs_Orientation.TopAbs_REVERSED else 1.0
        normal.Scale(sign / magnitude)
        relative = (
            float(point.X()) - torus.center[0],
            float(point.Y()) - torus.center[1],
            float(point.Z()) - torus.center[2],
        )
        along = math.fsum(relative[index] * torus.normal[index] for index in range(3))
        radial = tuple(relative[index] - along * torus.normal[index] for index in range(3))
        radial_length = math.hypot(*radial)
        if not math.isfinite(radial_length) or radial_length <= 0.0:
            return None
        tube_center = tuple(
            torus.center[index] + torus.major_radius * radial[index] / radial_length
            for index in range(3)
        )
        minor = tuple(
            float((point.X(), point.Y(), point.Z())[index]) - tube_center[index]
            for index in range(3)
        )
        dot = math.fsum(
            minor[index] * (normal.X(), normal.Y(), normal.Z())[index] for index in range(3)
        )
        if not math.isfinite(dot) or abs(dot) <= length_tol(torus.minor_radius, rel=1e-9):
            return None
        signs.add(1 if dot > 0.0 else -1)
    if len(signs) != 1:
        return None
    return "convex" if signs.pop() > 0 else "concave"


def _circular_proposal(
    component: frozenset[FaceNode],
    torus: _NativeTorus,
    graph: FaceGraph,
    surfaces: EffectiveSurfaceIndex,
) -> _BlendProposal | None:
    """Publish one complete toroidal edge path, or fail closed."""

    if not component or not _covers_complete_circle(graph, component):
        return None
    solid = graph.common_valid_solid(component)
    if solid is None:
        return None
    accounted: set[EdgeOccurrenceRef] = set()
    support_cache: dict[FaceNode, frozenset[FaceNode]] = {}
    support_sets: set[frozenset[FaceNode]] = set()
    sides: set[str] = set()
    for node in sorted(component, key=lambda item: item.index):
        side = _torus_side(graph, node, torus)
        if side is None:
            return None
        sides.add(side)
        for neighbour in sorted(graph.neighbours(node), key=lambda item: item.index):
            occurrences = graph.shared_occurrences(node, neighbour)
            if not occurrences or any(
                (ownership := graph.ownership(occurrence)) is None or ownership.solid is not solid
                for occurrence in occurrences
            ):
                return None
            accounted.update(half for occurrence in occurrences for half in occurrence.halves)
            if neighbour in component:
                internal_smooth = graph.arc(node, neighbour) == "smooth"
                if not internal_smooth:
                    return None
                continue
            spring_smooth = graph.arc(node, neighbour) == "smooth"
            if not spring_smooth:
                return None
            region = support_cache.get(neighbour)
            if region is None:
                region = _support_region(graph, surfaces, neighbour, excluded=component)
                for member in region:
                    support_cache[member] = region
            support_sets.add(region)
        face = graph.face(node)
        for occurrence in graph.edge_occurrences(node):
            if occurrence in accounted:
                continue
            edge = occurrence.edge
            if BRep_Tool.IsClosed_s(edge.wrapped, face.wrapped) or BRep_Tool.Degenerated_s(
                edge.wrapped
            ):
                continue
            return None
    if len(sides) != 1 or len(support_sets) != 2:
        return None
    if any(
        graph.common_valid_solid((*component, *support)) is not solid for support in support_sets
    ):
        return None
    support_facts: dict[SurfaceKind, AnalyticSurfaceFact] = {}
    for region in support_sets:
        facts = [surfaces.fact(node) for node in region]
        if not facts or any(not isinstance(fact, AnalyticSurfaceFact) for fact in facts):
            return None
        first = facts[0]
        assert isinstance(first, AnalyticSurfaceFact)
        if first.kind in support_facts or first.kind not in (
            SurfaceKind.PLANE,
            SurfaceKind.CYLINDER,
        ):
            return None
        support_facts[first.kind] = first
    if set(support_facts) != {SurfaceKind.PLANE, SurfaceKind.CYLINDER}:
        return None
    plane = support_facts[SurfaceKind.PLANE]
    cylinder = support_facts[SurfaceKind.CYLINDER]
    if 1.0 - abs(
        math.fsum(a * b for a, b in zip(plane.parameters[:3], torus.normal, strict=True))
    ) > SMOOTH_ARC_GAP or not _coaxial_axis_lines(
        torus.center,
        torus.normal,
        cylinder.parameters[:3],
        cylinder.parameters[3:6],
        tol=length_tol(max(torus.major_radius, cylinder.parameters[6]), rel=1e-9),
    ):
        return None
    nodes = tuple(sorted(component, key=lambda item: item.index))
    path = CircularBlendPath(
        center=(
            round(torus.center[0], 3),
            round(torus.center[1], 3),
            round(torus.center[2], 3),
        ),
        normal=torus.normal,
        radius=round(torus.major_radius, 3),
    )
    return _BlendProposal(
        Blend(radius=round(torus.minor_radius, 3), side=sides.pop(), path=path),
        nodes,
    )


def _parallel_planar_supports(
    chain: BlendChain,
    surfaces: EffectiveSurfaceIndex,
) -> bool:
    """Whether both spring supports prove parallel planes rather than an intersecting edge.

    A circular end joining two parallel slot walls is a constant-radius tangent cylinder, but it
    is not a rolling-ball treatment of an edge: the two support surfaces have no edge to round.
    Refuse only when both complete support regions prove this exact case. Curved or unavailable
    support geometry cannot establish the exclusion and remains governed by the complete chain
    contract.
    """

    normals: list[tuple[float, float, float]] = []
    for support in chain.supports:
        spring_nodes = {
            node for arc in chain.spring_arcs for node in arc.endpoints if node in support
        }
        facts = [surfaces.fact(node) for node in spring_nodes]
        if not facts or any(
            not isinstance(fact, AnalyticSurfaceFact) or fact.kind is not SurfaceKind.PLANE
            for fact in facts
        ):
            return False
        support_normals = [
            fact.parameters[:3] for fact in facts if isinstance(fact, AnalyticSurfaceFact)
        ]
        first = (support_normals[0][0], support_normals[0][1], support_normals[0][2])
        if any(
            1.0 - abs(math.fsum(a * b for a, b in zip(first, other, strict=True))) > SMOOTH_ARC_GAP
            for other in support_normals[1:]
        ):
            return False
        normals.append(first)
    return (
        1.0 - abs(math.fsum(a * b for a, b in zip(normals[0], normals[1], strict=True)))
        <= SMOOTH_ARC_GAP
    )


def _straight_path_anchor(
    graph: FaceGraph,
    nodes: tuple[FaceNode, ...],
    parameters: tuple[float, ...],
) -> tuple[float, float, float] | None:
    """Area-centre projected to the cylinder axis, invariant to face subdivision."""

    weighted: list[tuple[float, tuple[float, float, float]]] = []
    for node in nodes:
        props = GProp_GProps()
        BRepGProp.SurfaceProperties_s(graph.face(node).wrapped, props)
        area = float(props.Mass())
        centre = props.CentreOfMass()
        point = (float(centre.X()), float(centre.Y()), float(centre.Z()))
        if not math.isfinite(area) or area <= 0.0 or not all(map(math.isfinite, point)):
            return None
        weighted.append((area, point))
    total = math.fsum(area for area, _point in weighted)
    if not math.isfinite(total) or total <= 0.0:
        return None
    centre = tuple(
        math.fsum(area * point[index] for area, point in weighted) / total for index in range(3)
    )
    origin = parameters[:3]
    direction = parameters[3:6]
    relative = tuple(centre[index] - origin[index] for index in range(3))
    along = math.fsum(relative[index] * direction[index] for index in range(3))
    return (
        origin[0] + along * direction[0],
        origin[1] + along * direction[1],
        origin[2] + along * direction[2],
    )


def _proposal(
    chain: BlendChain,
    graph: FaceGraph,
    surfaces: EffectiveSurfaceIndex,
) -> _BlendProposal | None:
    if chain.side == "concave" and _parallel_planar_supports(chain, surfaces):
        return None
    nodes = tuple(sorted(chain.blend_nodes, key=lambda node: node.index))
    if not nodes:
        return None
    fact = surfaces.fact(nodes[0])
    if not isinstance(fact, AnalyticSurfaceFact) or len(fact.parameters) != 7:
        return None
    direction = fact.parameters[3:6]
    canonical = _canonical_direction(direction)
    anchor = _straight_path_anchor(graph, nodes, fact.parameters)
    if anchor is None:
        return None
    record = Blend(
        radius=round(chain.radius, 3),
        side=chain.side,
        path=StraightBlendPath(
            at=(round(anchor[0], 3), round(anchor[1], 3), round(anchor[2], 3)),
            direction=canonical,
        ),
    )
    return _BlendProposal(record, nodes)


def recognise_blends(part: Part) -> list[Blend]:
    """Recognise complete native straight and circular Blend paths in *part*."""

    return _discover_blends(part)


def _discover_blends(
    part: Part,
    *,
    graph: FaceGraph | None = None,
    surfaces: EffectiveSurfaceIndex | None = None,
    writer: EvidenceWriter | None = None,
) -> list[Blend]:
    """Shared writer-free/writer-enabled Blend discovery core."""

    if graph is None:
        graph = writer.graph if writer is not None else FaceGraph(part)
    if writer is not None and writer.graph is not graph:
        raise ValueError("blend graph and evidence writer belong to different runs")
    surfaces = EffectiveSurfaceIndex(graph) if surfaces is None else surfaces
    if surfaces.run_token is not graph.run_token:
        raise ValueError("blend graph and surface index belong to different runs")
    straight_proposals = [
        proposal
        for chain in BlendCollapseIndex(graph, surfaces).chains()
        if (proposal := _proposal(chain, graph, surfaces)) is not None
    ]
    circular_proposals = [
        proposal
        for component, torus in _toroidal_components(graph)
        if (proposal := _circular_proposal(component, torus, graph, surfaces)) is not None
    ]
    proposals = [*straight_proposals, *circular_proposals]
    proposals.sort(
        key=lambda proposal: (
            _path_sort_key(proposal.record.path),
            proposal.record.radius,
            proposal.record.side,
        )
    )
    if writer is not None:
        for proposal in proposals:
            if writer.graph.common_valid_solid(proposal.nodes) is None:
                raise ValueError("blend defining faces do not belong to one valid solid")
        for proposal in proposals:
            writer.add_defining(
                proposal.record,
                proposal.nodes,
                family=FamilyId.BLENDS,
            )
    return [proposal.record for proposal in proposals]


__all__ = ["Blend", "CircularBlendPath", "StraightBlendPath", "recognise_blends"]
