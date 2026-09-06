# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Private immutable support-bridge view over original graph provenance."""

from __future__ import annotations

import math
from collections import deque
from collections.abc import Iterable
from dataclasses import dataclass
from enum import Enum
from typing import cast

from OCP.BRep import BRep_Tool

from quiddity._adjacency import (
    ArcKind,
    FaceGraphQuery,
    FaceNode,
    GraphRunToken,
    SharedEdgeOccurrenceRef,
    SmoothSide,
    SolidRef,
)
from quiddity._analytic_surfaces import SurfaceKind, equivalent_parameters
from quiddity._effective_surfaces import (
    AnalyticSurfaceFact,
    EffectiveSurfaceQuery,
    OrientationCapability,
    SurfaceProvenance,
)


class BlendRefusalReason(Enum):
    UNSUPPORTED_SURFACE = "unsupported-surface"
    INCOMPLETE_BOUNDARY = "incomplete-boundary"
    BRANCHING_OR_CYCLE = "branching-or-cycle"
    MIXED_RADIUS_OR_SIDE = "mixed-radius-or-side"
    AMBIGUOUS_SUPPORT = "ambiguous-support"
    OWNERSHIP_UNPROVEN = "ownership-unproven"
    INVALID_LOCAL_SCALE = "invalid-local-scale"
    OVERLAPPING_COMPONENT = "overlapping-component"


@dataclass(frozen=True, eq=False, slots=True)
class OriginalArcRef:
    endpoints: tuple[FaceNode, FaceNode]
    occurrence: SharedEdgeOccurrenceRef


@dataclass(frozen=True, slots=True)
class FrozenProvenance:
    nodes: frozenset[FaceNode]
    arcs: tuple[OriginalArcRef, ...]


@dataclass(frozen=True, eq=False, slots=True)
class BlendChain:
    blend_nodes: frozenset[FaceNode]
    supports: tuple[frozenset[FaceNode], frozenset[FaceNode]]
    spring_arcs: tuple[OriginalArcRef, ...]
    internal_arcs: tuple[OriginalArcRef, ...]
    terminal_arcs: tuple[OriginalArcRef, ...]
    side: SmoothSide
    radius: float
    solid: SolidRef


@dataclass(frozen=True, eq=False, slots=True)
class RefusedBlendComponent:
    nodes: frozenset[FaceNode]
    reason: BlendRefusalReason


BlendDiscoveryResult = BlendChain | RefusedBlendComponent


@dataclass(frozen=True, eq=False, slots=True)
class LogicalNode:
    sources: frozenset[FaceNode]


@dataclass(frozen=True, eq=False, slots=True)
class LogicalArc:
    endpoints: tuple[LogicalNode, LogicalNode]
    kind: ArcKind
    synthetic: bool


def _physical_length(occurrences: tuple[OriginalArcRef, ...]) -> float:
    seen: list = []
    lengths: list[float] = []
    for arc in occurrences:
        edge = arc.occurrence.edge
        if BRep_Tool.Degenerated_s(edge.wrapped):
            continue
        if any(edge.wrapped.IsSame(other.wrapped) for other in seen):
            continue
        seen.append(edge)
        lengths.append(float(edge.length))
    return math.fsum(lengths)


def _node_key(node: FaceNode) -> int:
    """Stable run-local order owned by the original graph."""

    return node.index


def _arc_key(arc: OriginalArcRef) -> tuple[int, int, int, int, int, int]:
    """Stable occurrence order without consulting kernel object hashes."""

    left, right = arc.occurrence.halves
    return (
        arc.endpoints[0].index,
        arc.endpoints[1].index,
        left.wire_ordinal,
        left.ordinal,
        right.wire_ordinal,
        right.ordinal,
    )


def _ordered_arcs(occurrences: Iterable[OriginalArcRef]) -> tuple[OriginalArcRef, ...]:
    return tuple(sorted(occurrences, key=_arc_key))


def _edge_groups(
    occurrences: tuple[OriginalArcRef, ...],
) -> tuple[tuple[OriginalArcRef, ...], ...]:
    """Exact vertex-connected components, independent of occurrence traversal order."""

    if not occurrences:
        return ()
    occurrences = _ordered_arcs(occurrences)
    edges = [arc.occurrence.edge for arc in occurrences]
    remaining = set(range(len(edges)))
    groups: list[tuple[OriginalArcRef, ...]] = []
    while remaining:
        first = min(remaining)
        remaining.remove(first)
        pending = {first}
        reached: set[int] = set()
        while pending:
            at = min(pending)
            pending.remove(at)
            if at in reached:
                continue
            reached.add(at)
            vertices = edges[at].vertices()
            for other_at in tuple(remaining):
                if any(
                    left.wrapped.IsSame(right.wrapped)
                    for left in vertices
                    for right in edges[other_at].vertices()
                ):
                    remaining.remove(other_at)
                    pending.add(other_at)
        groups.append(tuple(occurrences[at] for at in sorted(reached)))
    return tuple(sorted(groups, key=lambda group: _arc_key(group[0])))


def _one_nonbranching_edge_group(occurrences: tuple[OriginalArcRef, ...]) -> bool:
    """Whether exact physical edges form one open, connected, nonbranching path."""

    groups = _edge_groups(occurrences)
    if len(groups) != 1:
        return False
    edges = [arc.occurrence.edge for arc in groups[0]]
    vertex_groups: list[tuple] = []
    for edge in edges:
        for vertex in edge.vertices():
            for group_at, (representative, count) in enumerate(vertex_groups):
                if vertex.wrapped.IsSame(representative.wrapped):
                    vertex_groups[group_at] = (representative, count + 1)
                    break
            else:
                vertex_groups.append((vertex, 1))
    counts = [count for _, count in vertex_groups]
    return counts.count(1) == 2 and all(count in (1, 2) for count in counts)


class BlendCollapseIndex:
    """Discover frozen neutral blend-chain occurrences once for one original graph."""

    def __init__(self, graph: FaceGraphQuery, surfaces: EffectiveSurfaceQuery) -> None:
        if graph.run_token is not surfaces.run_token:
            raise ValueError("blend graph and surface query belong to different runs")
        self._graph = graph
        self._surfaces = surfaces
        self._run_token: GraphRunToken = graph.run_token
        self._results: tuple[BlendDiscoveryResult, ...] | None = None
        self._issued_chains: dict[BlendChain, tuple] = {}
        self._issued_original_arcs: dict[OriginalArcRef, tuple] = {}
        self._issued_refusals: dict[RefusedBlendComponent, tuple] = {}
        self._view_issuer = object()

    @property
    def run_token(self) -> GraphRunToken:
        return self._run_token

    def results(self) -> tuple[BlendDiscoveryResult, ...]:
        if self._results is None:
            self._results = self._discover()
        for result in self._results:
            if isinstance(result, BlendChain):
                self._validate_chain(result)
            else:
                self._validate_refusal(result)
        return self._results

    def chains(self) -> tuple[BlendChain, ...]:
        return tuple(result for result in self.results() if isinstance(result, BlendChain))

    def view(self, selected: Iterable[BlendChain] = ()) -> CollapsedGraphView:
        return CollapsedGraphView(self, tuple(selected), _issuer=self._view_issuer)

    def _validate_selection(self, selected: tuple[BlendChain, ...]) -> tuple[BlendChain, ...]:
        seen_blends: set[FaceNode] = set()
        seen_arcs: set[SharedEdgeOccurrenceRef] = set()
        for chain in selected:
            self._validate_chain(chain)
            if seen_blends.intersection(chain.blend_nodes):
                raise ValueError("selected blend chains overlap")
            occurrences = {
                arc.occurrence
                for arc in (*chain.spring_arcs, *chain.internal_arcs, *chain.terminal_arcs)
            }
            if seen_arcs.intersection(occurrences):
                raise ValueError("selected blend chains share original arcs")
            seen_blends.update(chain.blend_nodes)
            seen_arcs.update(occurrences)
        return tuple(
            sorted(
                selected,
                key=lambda chain: (
                    min(node.index for node in chain.blend_nodes),
                    tuple(_arc_key(arc) for arc in chain.spring_arcs),
                    tuple(_arc_key(arc) for arc in chain.internal_arcs),
                    tuple(_arc_key(arc) for arc in chain.terminal_arcs),
                ),
            )
        )

    def _fact(self, node: FaceNode) -> AnalyticSurfaceFact | None:
        fact = self._surfaces.fact(node)
        if not self._graph.owns(node):
            raise ValueError("surface query was asked with a foreign graph node")
        if not isinstance(fact, AnalyticSurfaceFact) or fact.node is not node:
            return None
        if (
            fact.provenance is not SurfaceProvenance.NATIVE
            or fact.orientation is not OrientationCapability.NATIVE_ORIENTED
        ):
            return None
        return fact

    def _cylinder(self, node: FaceNode) -> AnalyticSurfaceFact | None:
        fact = self._fact(node)
        return fact if fact is not None and fact.kind is SurfaceKind.CYLINDER else None

    def _native_neutral(self, a: FaceNode, b: FaceNode) -> bool:
        legacy_smooth = self._graph.arc(a, b) == "smooth"
        neutral_side = self._graph.smooth_side(a, b) == "neutral"
        if not (legacy_smooth and neutral_side):
            return False
        left, right = self._fact(a), self._fact(b)
        proved_same_kind = left is not None and right is not None and left.kind is right.kind
        # F2's neutral fact is already the sole native-continuation certificate. Repeating its
        # parameter comparison with an individual edge/face scale here would make F3 eligibility
        # depend on topological subdivision; the complete-component aggregate scale is applied
        # once below to the cylindrical chain itself.
        return proved_same_kind

    def _cylinder_components(self) -> tuple[frozenset[FaceNode], ...]:
        pending = {node for node in self._graph.nodes if self._cylinder(node) is not None}
        components: list[frozenset[FaceNode]] = []
        while pending:
            first = min(pending, key=_node_key)
            pending.remove(first)
            found = {first}
            queue = deque((first,))
            while queue:
                current = queue.popleft()
                for neighbour in sorted(self._graph.neighbours(current), key=_node_key):
                    if neighbour in pending and self._native_neutral(current, neighbour):
                        pending.remove(neighbour)
                        found.add(neighbour)
                        queue.append(neighbour)
            components.append(frozenset(found))
        return tuple(sorted(components, key=lambda nodes: min(node.index for node in nodes)))

    def _support_region(
        self, first: FaceNode, *, excluded: frozenset[FaceNode], solid: SolidRef
    ) -> frozenset[FaceNode]:
        found = {first}
        queue = deque((first,))
        while queue:
            current = queue.popleft()
            for neighbour in sorted(self._graph.neighbours(current), key=_node_key):
                if neighbour in found or neighbour in excluded:
                    continue
                if self._native_neutral(current, neighbour):
                    occurrences = self._graph.shared_occurrences(current, neighbour)
                    if not occurrences or any(
                        (ownership := self._graph.ownership(occurrence)) is None
                        or ownership.solid is not solid
                        for occurrence in occurrences
                    ):
                        continue
                    found.add(neighbour)
                    queue.append(neighbour)
        return frozenset(found)

    def _arc_refs(self, a: FaceNode, b: FaceNode) -> tuple[OriginalArcRef, ...]:
        found = _ordered_arcs(
            OriginalArcRef(item.endpoints, item) for item in self._graph.shared_occurrences(a, b)
        )
        for arc in found:
            self._issued_original_arcs[arc] = (arc.endpoints, arc.occurrence)
        return found

    def _validate_original_arc(self, arc: OriginalArcRef) -> None:
        snapshot = self._issued_original_arcs.get(arc)
        if snapshot is None:
            raise ValueError("original arc was not issued by this blend index")
        endpoints, issued_occurrence = snapshot
        occurrence = arc.occurrence
        # ``ownership`` first revalidates the graph-issued occurrence even when ownership is
        # unavailable, which is the only read boundary this neutral value needs.
        self._graph.ownership(occurrence)
        if (
            occurrence is not issued_occurrence
            or any(
                actual is not expected
                for actual, expected in zip(arc.endpoints, endpoints, strict=True)
            )
            or any(
                actual is not expected
                for actual, expected in zip(arc.endpoints, occurrence.endpoints, strict=True)
            )
        ):
            raise ValueError("original arc endpoints changed after issuance")

    def _refuse(
        self, component: frozenset[FaceNode], reason: BlendRefusalReason
    ) -> RefusedBlendComponent:
        refusal = RefusedBlendComponent(component, reason)
        self._issued_refusals[refusal] = (component, reason)
        return refusal

    def _validate_refusal(self, refusal: RefusedBlendComponent) -> None:
        snapshot = self._issued_refusals.get(refusal)
        if snapshot is None or refusal.nodes != snapshot[0] or refusal.reason is not snapshot[1]:
            raise ValueError("blend refusal changed after issuance")
        if not all(self._graph.owns(node) for node in refusal.nodes):
            raise ValueError("blend refusal contains a changed graph node")

    def _classify(self, component: frozenset[FaceNode]) -> BlendDiscoveryResult:
        internal: list[OriginalArcRef] = []
        spring_neighbours: dict[FaceNode, dict[FaceNode, list[OriginalArcRef]]] = {
            node: {} for node in component
        }
        terminal_entries: list[tuple[FaceNode, FaceNode, OriginalArcRef]] = []
        sides: set[SmoothSide] = set()
        solid = None
        internal_degree = {node: 0 for node in component}
        accounted_halves: set = set()
        for node in sorted(component, key=_node_key):
            for neighbour in sorted(self._graph.neighbours(node), key=_node_key):
                if neighbour in component and node.index > neighbour.index:
                    continue
                refs = self._arc_refs(node, neighbour)
                if not refs:
                    return self._refuse(component, BlendRefusalReason.INCOMPLETE_BOUNDARY)
                for ref in refs:
                    accounted_halves.update(ref.occurrence.halves)
                    ownership = self._graph.ownership(ref.occurrence)
                    if ownership is None:
                        return self._refuse(component, BlendRefusalReason.OWNERSHIP_UNPROVEN)
                    if solid is None:
                        solid = ownership.solid
                    elif ownership.solid is not solid:
                        return self._refuse(component, BlendRefusalReason.OWNERSHIP_UNPROVEN)
                if neighbour in component:
                    if not self._native_neutral(node, neighbour):
                        return self._refuse(component, BlendRefusalReason.MIXED_RADIUS_OR_SIDE)
                    if not _one_nonbranching_edge_group(refs):
                        return self._refuse(component, BlendRefusalReason.BRANCHING_OR_CYCLE)
                    internal.extend(refs)
                    internal_degree[node] += 1
                    internal_degree[neighbour] += 1
                    continue
                side = self._graph.smooth_side(node, neighbour)
                if self._graph.arc(node, neighbour) == "smooth" and side in ("convex", "concave"):
                    spring_neighbours[node].setdefault(neighbour, []).extend(refs)
                    sides.add(side)
                    continue
                if self._graph.arc(node, neighbour) == "smooth":
                    return self._refuse(component, BlendRefusalReason.INCOMPLETE_BOUNDARY)
                terminal_entries.extend((node, neighbour, ref) for ref in refs)

        for node in sorted(component, key=_node_key):
            face = self._graph.face(node)
            for occurrence in self._graph.edge_occurrences(node):
                if occurrence in accounted_halves:
                    continue
                edge = occurrence.edge
                if BRep_Tool.IsClosed_s(edge.wrapped, face.wrapped) or BRep_Tool.Degenerated_s(
                    edge.wrapped
                ):
                    continue
                return self._refuse(component, BlendRefusalReason.INCOMPLETE_BOUNDARY)

        if len(component) == 1:
            if next(iter(internal_degree.values())) != 0:
                return self._refuse(component, BlendRefusalReason.BRANCHING_OR_CYCLE)
        else:
            degrees = tuple(internal_degree.values())
            if degrees.count(1) != 2 or any(value not in (1, 2) for value in degrees):
                return self._refuse(component, BlendRefusalReason.BRANCHING_OR_CYCLE)
        if len(sides) != 1:
            return self._refuse(component, BlendRefusalReason.MIXED_RADIUS_OR_SIDE)
        assert solid is not None
        support_cache: dict[FaceNode, frozenset[FaceNode]] = {}
        spring_by_patch: dict[FaceNode, dict[frozenset[FaceNode], list[OriginalArcRef]]] = {
            node: {} for node in component
        }
        for node, neighbours in spring_neighbours.items():
            for neighbour, spring_refs in neighbours.items():
                region = support_cache.get(neighbour)
                if region is None:
                    region = self._support_region(neighbour, excluded=component, solid=solid)
                    for member in region:
                        support_cache[member] = region
                spring_by_patch[node].setdefault(region, []).extend(spring_refs)
        support_sets = {region for groups in spring_by_patch.values() for region in groups}
        if len(support_sets) != 2 or any(
            set(groups) != support_sets for groups in spring_by_patch.values()
        ):
            return self._refuse(component, BlendRefusalReason.AMBIGUOUS_SUPPORT)
        if any(
            not _one_nonbranching_edge_group(tuple(arcs))
            for groups in spring_by_patch.values()
            for arcs in groups.values()
        ):
            return self._refuse(component, BlendRefusalReason.BRANCHING_OR_CYCLE)

        terminal_groups_arcs = _edge_groups(tuple(arc for _, _, arc in terminal_entries))
        if len(terminal_groups_arcs) != 2:
            return self._refuse(component, BlendRefusalReason.INCOMPLETE_BOUNDARY)
        path_ends = (
            component
            if len(component) == 1
            else frozenset(node for node, degree in internal_degree.items() if degree == 1)
        )
        attached_ends: set[FaceNode] = set()
        for group in terminal_groups_arcs:
            if not _one_nonbranching_edge_group(group):
                return self._refuse(component, BlendRefusalReason.BRANCHING_OR_CYCLE)
            attached = {
                blend
                for blend, _, arc in terminal_entries
                if any(arc is member for member in group)
            }
            if len(attached) != 1 or not attached <= path_ends:
                return self._refuse(component, BlendRefusalReason.INCOMPLETE_BOUNDARY)
            attached_ends.update(attached)
        if attached_ends != set(path_ends):
            return self._refuse(component, BlendRefusalReason.INCOMPLETE_BOUNDARY)

        supports = tuple(
            sorted(support_sets, key=lambda region: min(node.index for node in region))
        )
        spring_groups = tuple(
            _ordered_arcs(
                arc
                for node in sorted(component, key=_node_key)
                for arc in spring_by_patch[node][support]
            )
            for support in supports
        )
        first_fact = self._cylinder(min(component, key=_node_key))
        assert first_fact is not None
        radius = first_fact.parameters[-1]
        local_values = [
            radius,
            *(_physical_length(group) for group in (*spring_groups, *terminal_groups_arcs)),
            math.sqrt(math.fsum(float(self._graph.face(node).area) for node in component)),
            *(
                math.sqrt(math.fsum(float(self._graph.face(node).area) for node in support))
                for support in supports
            ),
        ]
        if not all(math.isfinite(value) and value > 0.0 for value in local_values):
            return self._refuse(component, BlendRefusalReason.INVALID_LOCAL_SCALE)
        local = min(local_values)
        if any(
            (fact := self._cylinder(node)) is None
            or not equivalent_parameters(
                SurfaceKind.CYLINDER, first_fact.parameters, fact.parameters, local=local
            )
            for node in component
        ):
            return self._refuse(component, BlendRefusalReason.MIXED_RADIUS_OR_SIDE)

        side = sides.pop()
        assert side in ("convex", "concave")
        chain = BlendChain(
            blend_nodes=component,
            supports=(supports[0], supports[1]),
            spring_arcs=_ordered_arcs(arc for group in spring_groups for arc in group),
            internal_arcs=_ordered_arcs(internal),
            terminal_arcs=_ordered_arcs(arc for group in terminal_groups_arcs for arc in group),
            side=side,
            radius=radius,
            solid=solid,
        )
        self._issued_chains[chain] = (
            chain.blend_nodes,
            chain.supports,
            chain.spring_arcs,
            chain.internal_arcs,
            chain.terminal_arcs,
            chain.side,
            chain.radius,
            chain.solid,
        )
        return chain

    def _discover(self) -> tuple[BlendDiscoveryResult, ...]:
        results = [self._classify(component) for component in self._cylinder_components()]
        chains = [result for result in results if isinstance(result, BlendChain)]
        conflicted: set[BlendChain] = set()
        for at, left in enumerate(chains):
            for right in chains[at + 1 :]:
                if left.blend_nodes.intersection(right.blend_nodes):
                    conflicted.update((left, right))
                    continue
                for left_support in left.supports:
                    for right_support in right.supports:
                        if left_support != right_support and left_support.intersection(
                            right_support
                        ):
                            conflicted.update((left, right))
        if not conflicted:
            return tuple(results)
        final: list[BlendDiscoveryResult] = []
        for result in results:
            if isinstance(result, BlendChain) and result in conflicted:
                self._issued_chains.pop(result, None)
                final.append(
                    self._refuse(result.blend_nodes, BlendRefusalReason.OVERLAPPING_COMPONENT)
                )
            else:
                final.append(result)
        return tuple(final)

    def _validate_chain(self, chain: BlendChain) -> None:
        snapshot = self._issued_chains.get(chain)
        if snapshot is None:
            raise ValueError("blend chain was not issued by this index")
        actual = (
            chain.blend_nodes,
            chain.supports,
            chain.spring_arcs,
            chain.internal_arcs,
            chain.terminal_arcs,
            chain.side,
            chain.radius,
            chain.solid,
        )
        if (
            any(
                value != expected
                for value, expected in zip(actual[:-1], snapshot[:-1], strict=True)
            )
            or actual[-1] is not snapshot[-1]
        ):
            raise ValueError("blend chain changed after issuance")
        if not all(
            self._graph.owns(node)
            for node in (*chain.blend_nodes, *chain.supports[0], *chain.supports[1])
        ):
            raise ValueError("blend chain contains a changed graph node")
        owned_solids: set[SolidRef] = set()
        for arc in (*chain.spring_arcs, *chain.internal_arcs, *chain.terminal_arcs):
            self._validate_original_arc(arc)
            ownership = self._graph.ownership(arc.occurrence)
            if ownership is None:
                raise ValueError("blend chain ownership changed after issuance")
            owned_solids.add(ownership.solid)
        if owned_solids != {chain.solid}:
            raise ValueError("blend chain solid changed after issuance")


class CollapsedGraphView:
    """Explicit selected support bridges with complete original provenance."""

    def __init__(
        self,
        index: BlendCollapseIndex,
        selected: tuple[BlendChain, ...],
        *,
        _issuer: object,
    ) -> None:
        if _issuer is not index._view_issuer:
            raise ValueError("collapsed views must be issued by their blend index")
        selected = index._validate_selection(selected)
        self._index = index
        self._graph = index._graph
        self._selected = selected
        hidden = frozenset(node for chain in selected for node in chain.blend_nodes)
        support_sets: list[frozenset[FaceNode]] = []
        for chain in selected:
            for support in chain.supports:
                if support not in support_sets:
                    support_sets.append(support)
        covered = frozenset(node for support in support_sets for node in support)
        support_sets.sort(key=lambda sources: min(node.index for node in sources))
        sources = support_sets + [
            frozenset((node,))
            for node in self._graph.nodes
            if node not in hidden and node not in covered
        ]
        self._nodes = tuple(LogicalNode(source) for source in sources)
        self._node_provenance: dict[LogicalNode, FrozenProvenance] = {}
        self._issued_nodes: dict[LogicalNode, tuple] = {}
        for node in self._nodes:
            members = tuple(sorted(node.sources, key=_node_key))
            internal = _ordered_arcs(
                arc
                for at, left in enumerate(members)
                for right in members[at + 1 :]
                for arc in self._index._arc_refs(left, right)
            )
            node_value = FrozenProvenance(node.sources, internal)
            self._node_provenance[node] = node_value
            self._issued_nodes[node] = (node.sources, node_value.nodes, node_value.arcs)
        self._by_source = {source: node for node in self._nodes for source in node.sources}
        arcs: list[LogicalArc] = []
        arc_provenance: dict[LogicalArc, FrozenProvenance] = {}
        for at, left in enumerate(self._graph.nodes):
            if left in hidden:
                continue
            for right in self._graph.nodes[at + 1 :]:
                if right in hidden:
                    continue
                mapped_left, mapped_right = self._by_source[left], self._by_source[right]
                refs = self._index._arc_refs(left, right)
                if mapped_left is mapped_right:
                    continue
                kind = self._graph.arc(left, right)
                if kind is None:
                    continue
                for ref in refs:
                    arc = LogicalArc((mapped_left, mapped_right), kind, False)
                    arcs.append(arc)
                    arc_provenance[arc] = FrozenProvenance(frozenset((left, right)), (ref,))
        for chain in selected:
            logical_left, logical_right = (
                self._by_source[min(source, key=_node_key)] for source in chain.supports
            )
            bridge_kind: ArcKind = "convex" if chain.side == "convex" else "concave"
            arc = LogicalArc((logical_left, logical_right), bridge_kind, True)
            arcs.append(arc)
            arc_provenance[arc] = FrozenProvenance(
                frozenset((*chain.blend_nodes, *chain.supports[0], *chain.supports[1])),
                _ordered_arcs((*chain.spring_arcs, *chain.internal_arcs, *chain.terminal_arcs)),
            )
        self._arcs = tuple(arcs)
        self._issued_arcs = {
            arc: (
                arc.endpoints,
                arc.kind,
                arc.synthetic,
                arc_provenance[arc].nodes,
                arc_provenance[arc].arcs,
            )
            for arc in arcs
        }

    def logical_nodes(self) -> tuple[LogicalNode, ...]:
        for node in self._nodes:
            self._validate_node(node)
        return self._nodes

    def _validate_node(self, node: LogicalNode) -> None:
        snapshot = self._issued_nodes.get(node)
        if snapshot is None:
            raise ValueError("logical node is foreign or changed")
        sources, provenance_nodes, provenance_arcs = snapshot
        provenance = self._node_provenance[node]
        if (
            node.sources != sources
            or provenance.nodes != provenance_nodes
            or provenance.arcs != provenance_arcs
            or not all(self._graph.owns(source) for source in node.sources)
        ):
            raise ValueError("logical node is foreign or changed")
        for original in provenance_arcs:
            self._index._validate_original_arc(original)

    def _validate_arc(self, arc: LogicalArc) -> FrozenProvenance:
        snapshot = self._issued_arcs.get(arc)
        if snapshot is None:
            raise ValueError("logical arc was not issued by this view")
        endpoints, kind, synthetic, provenance_nodes, provenance_arcs = snapshot
        if (
            any(
                actual is not expected
                for actual, expected in zip(arc.endpoints, endpoints, strict=True)
            )
            or arc.kind != kind
            or arc.synthetic is not synthetic
        ):
            raise ValueError("logical arc changed after issuance")
        for endpoint in endpoints:
            self._validate_node(endpoint)
        provenance = FrozenProvenance(provenance_nodes, provenance_arcs)
        for original in provenance_arcs:
            self._index._validate_original_arc(original)
        if not all(self._graph.owns(node) for node in provenance_nodes):
            raise ValueError("logical arc provenance contains a changed graph node")
        return provenance

    def neighbours(self, node: LogicalNode) -> tuple[LogicalNode, ...]:
        self._validate_node(node)
        found: list[LogicalNode] = []
        for arc in self._arcs:
            self._validate_arc(arc)
            if arc.endpoints[0] is node and arc.endpoints[1] not in found:
                found.append(arc.endpoints[1])
            elif arc.endpoints[1] is node and arc.endpoints[0] not in found:
                found.append(arc.endpoints[0])
        return tuple(found)

    def arcs_between(self, a: LogicalNode, b: LogicalNode) -> tuple[LogicalArc, ...]:
        self._validate_node(a)
        self._validate_node(b)
        result = tuple(
            arc
            for arc in self._arcs
            if (arc.endpoints[0] is a and arc.endpoints[1] is b)
            or (arc.endpoints[0] is b and arc.endpoints[1] is a)
        )
        for arc in result:
            self._validate_arc(arc)
        return result

    def expand_node(self, node: LogicalNode) -> frozenset[FaceNode]:
        self._validate_node(node)
        return cast(frozenset[FaceNode], self._issued_nodes[node][0])

    def node_provenance(self, node: LogicalNode) -> FrozenProvenance:
        self._validate_node(node)
        _, nodes, arcs = self._issued_nodes[node]
        return FrozenProvenance(nodes, arcs)

    def expand_arc(self, arc: LogicalArc) -> FrozenProvenance:
        return self._validate_arc(arc)
