# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Recognition of bounded regular hexagonal bosses and whole-stock prisms.

The proven capability is intentionally narrow: principal-axis hexagons with six planar side
faces, opposed equal support planes, and unambiguous terminal caps. Other axes or polygon classes
fail closed until independent corpus evidence establishes their geometry contract.
"""

from __future__ import annotations

import math
from collections import Counter
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from typing import Any, TypeVar, cast

from quiddity._candidates import FamilyId
from quiddity._geometry import AXIS_ALIGNED_COS
from quiddity._geometry_evidence import GeometryEvidenceBridge
from quiddity._record import Record
from quiddity._typing import FaceLike, Part
from quiddity.experimental_geometry import (
    AnalyticSurface,
    BlendFact,
    FaceRef,
    GeometryGraph,
    SurfaceKind,
)

#: Whatever a caller keys its ring by: this module passes face nodes, the unit tests pass ints.
#: The two polygon helpers below never look inside it, which is the point -- they are geometry
#: over headings and have no business knowing a B-Rep exists.
_K = TypeVar("_K")

#: **A minimum-evidence threshold, not a tolerance — deliberately absolute (ADR 0008).**
#: Scaling it to the part makes a feature's existence depend on what surrounds it, so a small
#: feature on a large part disappears. Whether such a feature is worth dimensioning is consumer
#: policy, and ADR 0001 puts policy with the consumer; recognition reports it either way.
#: Also the minimum boss height and support span.
_TOL = 0.2

#: A boss side face is vertical: its normal has essentially no Z component. Looser than the
#: package's AXIS_ZERO_COS because an extruded prism's walls carry the sketch's angular noise,
#: and a side rejected here costs the whole ring.
_SIDE_VERTICAL_COS = 0.02


@dataclass(frozen=True, order=True)
class PolygonalBoss(Record):
    """A regular hexagonal principal-axis prism attached to a support face.

    The recogniser emits ``axis`` as ``"x"``, ``"y"`` or ``"z"`` and exactly
    ``side_count=6``. Other polygon classes require their own evidence before they
    become package capability. ``flat_directions`` preserve the ordered outward
    evidence that established the hexagon. ``flat_centres`` are real points on the
    defining side faces, so rendering can anchor a leader without reconstructing it
    from A/F.
    """

    axis: str
    center: tuple[float, float, float]
    side_count: int
    across_flats: float
    base: float
    top: float
    flat_directions: tuple[tuple[float, float, float], ...]
    flat_centres: tuple[tuple[float, float, float], ...]

    @property
    def height(self) -> float:
        return self.top - self.base


@dataclass(frozen=True, order=True)
class PolygonalStock(Record):
    """A whole solid proved to be a regular hexagonal prism.

    The recogniser emits one principal ``axis`` and exactly ``side_count=6``.
    This is deliberately distinct from :class:`PolygonalBoss`: its two caps terminate the
    complete solid, rather than one cap being an attachment to supporting material.
    """

    axis: str
    center: tuple[float, float, float]
    side_count: int
    across_flats: float
    base: float
    top: float
    flat_directions: tuple[tuple[float, float, float], ...]
    flat_centres: tuple[tuple[float, float, float], ...]

    @property
    def length(self) -> float:
        return self.top - self.base


@dataclass(frozen=True, slots=True)
class _PolygonalProposal:
    record: PolygonalBoss | PolygonalStock
    side_faces: tuple[FaceLike, ...]
    lower_cap: FaceLike
    upper_cap: FaceLike
    terminal_cap: FaceLike | None


@dataclass(frozen=True, slots=True)
class _CapSelection:
    """One unique terminal cap retained with the coordinate it establishes."""

    node: FaceRef
    coordinate: float

    @property
    def z(self) -> float:
        """Legacy private spelling retained for the Z-axis helper contract."""
        return self.coordinate


_AXIS_INDEX = {"x": 0, "y": 1, "z": 2}
_TRANSVERSE_INDICES = {"x": (1, 2), "y": (0, 2), "z": (0, 1)}


def _point_coordinate(point: Any, index: int) -> float:
    return float((point.X, point.Y, point.Z)[index])


def _rounded(value: float, digits: int) -> float:
    rounded = round(value, digits)
    return 0.0 if rounded == 0.0 else rounded


def _flat_direction(
    heading: tuple[float, float, float], transverse_indices: tuple[int, int]
) -> tuple[float, float, float]:
    values = [0.0, 0.0, 0.0]
    values[transverse_indices[0]] = _rounded(heading[0], 3)
    values[transverse_indices[1]] = _rounded(heading[1], 3)
    return (values[0], values[1], values[2])


def _connected_components(
    items: list[_K] | set[_K], joined: Callable[[_K, _K], bool]
) -> list[tuple[_K, ...]]:
    """Local policy walk; the facade supplies adjacency, not component semantics."""

    components: list[tuple[_K, ...]] = []
    unseen = set(items)
    while unseen:
        connected = {unseen.pop()}
        frontier = list(connected)
        while frontier:
            current = frontier.pop()
            attached = {other for other in unseen if joined(current, other)}
            unseen -= attached
            connected |= attached
            frontier.extend(attached)
        components.append(tuple(connected))
    return components


def _heading(graph: GeometryGraph, node: FaceRef) -> tuple[float, float, float]:
    """A side face's outward normal, known to exist.

    Every node that reaches the ring helpers came through `_vertical_side_faces`, which already
    refused a face whose normal will not evaluate. Saying that once here beats either guarding
    it at six call sites or writing six unreachable branches -- the graph cannot know the
    filter has run, but this module does.
    """

    return cast("tuple[float, float, float]", graph.normal(node))


def _cap_coordinate(
    graph: GeometryGraph,
    node: FaceRef,
    tol: float,
    *,
    axis_index: int,
    positive: bool,
    lower_than: float | None,
    higher_than: float | None,
) -> float | None:
    """The axial coordinate of *face* if it can serve as a terminal cap, else ``None``.

    A cap is planar, faces squarely along the selected principal axis in the required direction,
    sits at one axial coordinate, and lies on the correct side of the wall it terminates.
    """

    if not graph.is_planar(node):
        return None
    normal = graph.normal(node)
    if normal is None or (
        normal[axis_index] < AXIS_ALIGNED_COS
        if positive
        else normal[axis_index] > -AXIS_ALIGNED_COS
    ):
        return None
    coordinate_lo, coordinate_hi = graph.bounds(node)[axis_index]
    if coordinate_hi - coordinate_lo > tol:
        return None
    coordinate = (coordinate_lo + coordinate_hi) / 2
    if lower_than is not None and coordinate > lower_than + tol:
        return None
    if higher_than is not None and coordinate < higher_than - tol:
        return None
    return coordinate


def _cap_z(
    graph: GeometryGraph,
    node: FaceRef,
    tol: float,
    *,
    positive: bool,
    lower_than: float | None,
    higher_than: float | None,
) -> float | None:
    """Compatibility spelling for the Z-axis unit contract."""
    return _cap_coordinate(
        graph,
        node,
        tol,
        axis_index=2,
        positive=positive,
        lower_than=lower_than,
        higher_than=higher_than,
    )


def _common_cap(
    component: tuple[FaceRef, ...],
    graph: GeometryGraph,
    adjacent_to: Callable[[FaceRef], set[FaceRef]],
    tol: float,
    *,
    axis_index: int = 2,
    upper: bool,
    positive: bool,
    wall_lo: float,
    wall_hi: float,
) -> _CapSelection | None:
    """The single cap Z shared by every side of the ring, or ``None``.

    Each side must reach the end through exactly one neighbour — an ambiguous choice means the
    ring is not cleanly terminated — and those neighbours must then meet at exactly one cap
    face. Requiring exactly one at both steps is what makes this fail closed: a boss with two
    candidate tops is not a boss whose top we can name.
    """

    boundary: list[FaceRef] = []
    component_set = set(component)
    for side in component:
        choices = []
        for other in adjacent_to(side) - component_set:
            coordinate_lo, coordinate_hi = graph.bounds(other)[axis_index]
            reaches_end = (
                abs(coordinate_lo - wall_hi) <= tol
                if upper
                else abs(coordinate_hi - wall_lo) <= tol
            )
            if reaches_end:
                choices.append(other)
        if len(choices) != 1:
            return None
        boundary.append(choices[0])

    boundary_set = set(boundary)
    if len(boundary_set) == 1:
        candidates = boundary_set
    else:
        candidates = set.intersection(*(adjacent_to(face) for face in boundary_set))
        candidates -= component_set | boundary_set
    cap_selections = [
        _CapSelection(node, cap)
        for node in candidates
        if (
            cap := _cap_coordinate(
                graph,
                node,
                tol,
                axis_index=axis_index,
                positive=positive,
                lower_than=None if upper else wall_lo,
                higher_than=wall_hi if upper else None,
            )
        )
        is not None
    ]
    return cap_selections[0] if len(cap_selections) == 1 else None


def _side_rings(
    sides: list[FaceRef],
    graph: GeometryGraph,
    tol: float,
    shares_edge: Callable[[FaceRef, FaceRef], bool],
    *,
    axis_index: int = 2,
) -> list[tuple[FaceRef, ...]]:
    """Group side faces into rings: connected, and spanning the same axial range.

    Both conditions are needed. Sharing an edge alone would chain a boss into the plate it
    stands on; sharing a Z span alone would merge two separate bosses of equal height into one
    ring with twelve sides.
    """

    def same_span(i: FaceRef, j: FaceRef) -> bool:
        lo_i, hi_i = graph.bounds(i)[axis_index]
        lo_j, hi_j = graph.bounds(j)[axis_index]
        return abs(lo_i - lo_j) <= tol and abs(hi_i - hi_j) <= tol

    return _connected_components(sides, lambda i, j: same_span(i, j) and shares_edge(i, j))


def _principal_side_faces(graph: GeometryGraph, tol: float, *, axis_index: int) -> list[FaceRef]:
    """Planar faces perpendicular to the profile plane and long enough to be walls.

    Only the selection is this recogniser's; the normal and the bounding box it selects on come
    from the graph, which memoises them per face. Deriving them here meant a second copy of both
    for every face the module touched, and the map that held them was the ad hoc face graph this
    package now has one of.
    """

    sides: list[FaceRef] = []
    for node in graph.faces:
        if not graph.is_planar(node):
            continue
        normal = graph.normal(node)
        if normal is None or abs(normal[axis_index]) > _SIDE_VERTICAL_COS:
            continue
        coordinate_lo, coordinate_hi = graph.bounds(node)[axis_index]
        if coordinate_hi - coordinate_lo <= tol:
            continue
        sides.append(node)
    return sides


def _vertical_side_faces(graph: GeometryGraph, tol: float) -> list[FaceRef]:
    """Compatibility spelling for Z-axis side selection."""
    return _principal_side_faces(graph, tol, axis_index=2)


def _six_support_cycle_indices(
    pairs: tuple[frozenset[FaceRef], ...],
) -> tuple[int, ...]:
    """Indices belonging to disjoint exact six-edge/six-node degree-two components."""

    remaining = set(range(len(pairs)))
    selected: list[int] = []
    while remaining:
        seed = min(remaining)
        remaining.remove(seed)
        component = {seed}
        supports = set(pairs[seed])
        changed = True
        while changed:
            changed = False
            for at in tuple(remaining):
                if supports.intersection(pairs[at]):
                    remaining.remove(at)
                    component.add(at)
                    supports.update(pairs[at])
                    changed = True
        ordered = sorted(component)
        component_pairs = [pairs[at] for at in ordered]
        if len(ordered) != 6 or len(supports) != 6 or len(set(component_pairs)) != 6:
            continue
        if any(sum(node in pair for pair in component_pairs) != 2 for node in supports):
            continue
        selected.extend(ordered)
    return tuple(selected)


def _polygonal_boss_blend_bridges(
    graph: GeometryGraph,
    side_faces: list[FaceRef],
    tol: float,
    *,
    axis_index: int = 2,
) -> frozenset[frozenset[FaceRef]]:
    """Return only provenance-complete bridges for unambiguous six-support blend cycles."""

    side_set = set(side_faces)
    possible: list[tuple[FaceRef, frozenset[FaceRef]]] = []
    for node in graph.faces:
        if node in side_set:
            continue
        supports = side_set.intersection(graph.neighbours(node))
        if len(supports) != 2:
            continue
        left, right = tuple(supports)
        if any(
            abs(a - b) > tol
            for a, b in zip(
                graph.bounds(left)[axis_index],
                graph.bounds(right)[axis_index],
                strict=True,
            )
        ):
            continue
        possible.append((node, frozenset(supports)))

    def contains_six_cycle(pairs: list[frozenset[FaceRef]]) -> bool:
        possible_supports = set().union(*pairs) if pairs else set()
        return len(pairs) >= 6 and any(
            len(component) == 6 and sum(pair <= set(component) for pair in pairs) >= 6
            for component in _connected_components(
                possible_supports,
                lambda left, right: frozenset((left, right)) in pairs,
            )
        )

    possible_pairs = [pair for _node, pair in possible]
    if not contains_six_cycle(possible_pairs):
        return frozenset()

    cylindrical_pairs = [
        pair
        for node, pair in possible
        if isinstance(fact := graph.surface_fact(node), AnalyticSurface)
        and fact.kind is SurfaceKind.CYLINDER
    ]
    if not contains_six_cycle(cylindrical_pairs):
        return frozenset()
    eligible: list[tuple[BlendFact, FaceRef, FaceRef]] = []
    for chain in graph.blend_facts():
        if chain.side != "convex" or len(chain.blend_faces) != 1:
            continue
        if any(len(support) != 1 for support in chain.supports):
            continue
        left = next(iter(chain.supports[0]))
        right = next(iter(chain.supports[1]))
        support_facts = (graph.surface_fact(left), graph.surface_fact(right))
        if left is right or any(
            not isinstance(fact, AnalyticSurface) or fact.kind is not SurfaceKind.PLANE
            for fact in support_facts
        ):
            continue
        normals = (graph.normal(left), graph.normal(right))
        if any(
            normal is None or abs(normal[axis_index]) > _SIDE_VERTICAL_COS for normal in normals
        ):
            continue
        left_span = graph.bounds(left)[axis_index]
        right_span = graph.bounds(right)[axis_index]
        if any(abs(a - b) > tol for a, b in zip(left_span, right_span, strict=True)):
            continue
        eligible.append((chain, left, right))

    eligible_pairs = tuple(frozenset((left, right)) for _chain, left, right in eligible)
    selected_indices = _six_support_cycle_indices(eligible_pairs)
    selected = [eligible[at][0] for at in selected_indices]
    selected_pairs = [eligible_pairs[at] for at in selected_indices]

    if not selected:
        return frozenset()
    bridges = graph.collapsed_bridges(tuple(chain.ref for chain in selected))
    for chain, pair in zip(selected, selected_pairs, strict=True):
        left, right = tuple(pair)
        support_refs = frozenset((left, right))
        arcs = tuple(bridge for bridge in bridges if frozenset(bridge.supports) == support_refs)
        if len(arcs) != 1:
            raise ValueError("selected Polygonal Boss blend chain has no unique logical bridge")
        provenance = arcs[0].provenance
        expected_nodes = frozenset((*chain.blend_faces, *chain.supports[0], *chain.supports[1]))
        if provenance.faces != expected_nodes or Counter(provenance.boundary) != Counter(
            chain.boundary
        ):
            raise ValueError("selected Polygonal Boss bridge lost original provenance")
    return frozenset(selected_pairs)


def _regular_ring_order(
    component: tuple[_K, ...],
    headings: Mapping[_K, tuple[float, float, float]],
    angle_tol: float,
) -> tuple[_K, ...] | None:
    """Order a side ring by heading, or reject it as not a regular polygon.

    Two independent proofs, both needed: the headings are evenly spaced, and each side faces
    directly away from the one opposite it. Even spacing alone admits a ring that spirals; the
    opposed test alone admits an irregular polygon whose pairs happen to be parallel.
    """

    side_count = len(component)

    def heading_angle(key: _K) -> float:
        across, along, _ = headings[key]
        return math.atan2(along, across)

    ordered = tuple(sorted(component, key=heading_angle))
    angles = [heading_angle(i) % (2 * math.pi) for i in ordered]
    gaps = [(angles[(i + 1) % side_count] - angles[i]) % (2 * math.pi) for i in range(side_count)]
    expected_gap = 2 * math.pi / side_count
    if any(abs(gap - expected_gap) > angle_tol for gap in gaps):
        return None
    opposite = side_count // 2
    if any(
        headings[ordered[i]][0] * headings[ordered[i + opposite]][0]
        + headings[ordered[i]][1] * headings[ordered[i + opposite]][1]
        > -math.cos(angle_tol)
        for i in range(opposite)
    ):
        return None
    return ordered


def _ring_profile(
    ordered: tuple[_K, ...],
    headings: Mapping[_K, tuple[float, float, float]],
    centres: list,
    tol: float,
    *,
    transverse_indices: tuple[int, int] = (0, 1),
) -> tuple[float, float, float] | None:
    """The ring's axis ``(x, y)`` and across-flats, or ``None`` if it is not one prism.

    Each opposed pair of side planes defines a midplane containing the axis. Six such planes
    over-determine a point, so the axis is the least-squares intersection rather than any one
    pair's — which keeps a single noisy face from moving the reported centre.

    The support distances then have to agree: every side the same distance out, and every
    opposed pair the same distance apart. Disagreement means an irregular polygon, and a
    non-positive support means the walls face inward, which is a recess rather than a boss.
    """

    side_count = len(ordered)
    opposite = side_count // 2
    plane_offsets = [
        headings[index][0] * _point_coordinate(point, transverse_indices[0])
        + headings[index][1] * _point_coordinate(point, transverse_indices[1])
        for index, point in zip(ordered, centres, strict=True)
    ]
    midplanes = [
        (
            headings[ordered[i]][0],
            headings[ordered[i]][1],
            (plane_offsets[i] - plane_offsets[i + opposite]) / 2,
        )
        for i in range(opposite)
    ]
    sxx = sum(nx * nx for nx, _ny, _offset in midplanes)
    sxy = sum(nx * ny for nx, ny, _offset in midplanes)
    syy = sum(ny * ny for _nx, ny, _offset in midplanes)
    bx = sum(nx * offset for nx, _ny, offset in midplanes)
    by = sum(ny * offset for _nx, ny, offset in midplanes)
    determinant = sxx * syy - sxy * sxy
    # Six normals that passed the near-60-degree ring gate necessarily span the plane.
    cx = (bx * syy - by * sxy) / determinant
    cy = (sxx * by - sxy * bx) / determinant

    supports = [
        offset - headings[index][0] * cx - headings[index][1] * cy
        for index, offset in zip(ordered, plane_offsets, strict=True)
    ]
    if min(supports) <= tol:
        return None  # inward-facing walls describe a recess, not material projecting out
    across_values = [supports[i] + supports[i + opposite] for i in range(opposite)]
    across = sum(across_values) / len(across_values)
    if max(abs(value - across) for value in across_values) > tol:
        return None
    if max(abs(value - across / 2) for value in supports) > tol:
        return None
    return cx, cy, across


def _recognise_one(
    part: Part,
    *,
    tol: float | None,
    angle_tol: float,
    whole_stock: bool = False,
    axis: str = "z",
    graph: GeometryGraph | None = None,
) -> list[_PolygonalProposal]:
    tol = _TOL if tol is None else tol
    axis_index = _AXIS_INDEX[axis]
    transverse_indices = _TRANSVERSE_INDICES[axis]
    # The graph holds the face inventory, the adjacency and the per-face attributes this
    # module used to keep three private maps for. Its accessors memoise on first ask, which is
    # the property the hand-rolled cache here existed for: only the vertical sides and the few
    # faces bounding a ring are ever asked about, and resolving the rest measured at more than
    # half of this recogniser's total time on the corpus.
    #
    # Memoising is also why an aggregate should hand its own graph down rather than let this
    # build a second one over the same faces: the run has already resolved some of them.
    if graph is None:
        graph = GeometryGraph(part)
    else:
        # A graph over the wrong solid would not raise here on its own -- it would answer
        # questions about the wrong faces, and a boss found on one solid would be reported for
        # another. Checked rather than trusted, as `_rings` checks its own caller.
        faces = tuple(part.faces())
        resolved = {graph.ref(face) for face in faces}
        if len(resolved) != len(faces) or len(resolved) != len(graph):
            raise ValueError("supplied Polygonal Boss graph does not exactly match the part")

    sides = _principal_side_faces(graph, tol, axis_index=axis_index)
    if len(sides) < 6:
        return []
    blend_bridges = (
        frozenset()
        if whole_stock
        else _polygonal_boss_blend_bridges(graph, sides, tol, axis_index=axis_index)
    )

    def shares_edge(i: FaceRef, j: FaceRef) -> bool:
        return j in graph.neighbours(i) or frozenset((i, j)) in blend_bridges

    components = _side_rings(sides, graph, tol, shares_edge, axis_index=axis_index)

    def adjacent_to(node: FaceRef) -> set[FaceRef]:
        # A fresh set each time: `_common_cap` subtracts from what it gets back, and the graph
        # hands out a tuple precisely so one ring's bookkeeping cannot corrupt the next one's.
        return set(graph.neighbours(node))

    found: list[_PolygonalProposal] = []
    for component in components:
        side_count = len(component)
        # The accepted corpus proves hexagonal bosses. Broader polygon classes need their own
        # corpus evidence before automatic recognition can claim them.
        if side_count != 6:
            continue
        # Whole stock is intentionally the exact-prism class: one closed solid made only
        # from this side ring and its two terminal caps. Attached bosses, recesses, holes,
        # chamfers and assemblies need different ownership/evidence.
        if whole_stock and len(graph) != side_count + 2:
            continue
        component_set = set(component)
        if any(
            len({other for other in component_set if other != side and shares_edge(side, other)})
            != 2
            for side in component
        ):
            continue

        # Sourced from the graph's memo, not re-derived -- but handed on as a plain mapping,
        # because these two are pure geometry over headings and have their own unit tests.
        # Making them take the graph would have coupled a polygon calculation to a B-Rep.
        headings = {
            node: (
                _heading(graph, node)[transverse_indices[0]],
                _heading(graph, node)[transverse_indices[1]],
                0.0,
            )
            for node in component
        }
        ordered = _regular_ring_order(component, headings, angle_tol)
        if ordered is None:
            continue
        centres = [graph.face(i).center() for i in ordered]
        profile = _ring_profile(
            ordered,
            headings,
            centres,
            tol,
            transverse_indices=transverse_indices,
        )
        if profile is None:
            continue
        cx, cy, across = profile

        wall_lo = sum(graph.bounds(i)[axis_index][0] for i in component) / side_count
        wall_hi = sum(graph.bounds(i)[axis_index][1] for i in component) / side_count
        cap_directions = (True,) if whole_stock else (True, False)
        cap_pairs = []
        for positive in cap_directions:
            base = _common_cap(
                component,
                graph,
                adjacent_to,
                tol,
                axis_index=axis_index,
                upper=False,
                positive=False if whole_stock else positive,
                wall_lo=wall_lo,
                wall_hi=wall_hi,
            )
            top = _common_cap(
                component,
                graph,
                adjacent_to,
                tol,
                axis_index=axis_index,
                upper=True,
                positive=True if whole_stock else positive,
                wall_lo=wall_lo,
                wall_hi=wall_hi,
            )
            if base is not None and top is not None:
                cap_pairs.append((base, top, positive))
        if len(cap_pairs) != 1:
            continue
        base, top, positive = cap_pairs[0]
        if base is None or top is None or top.coordinate - base.coordinate <= tol:
            continue
        if whole_stock and (
            abs(base.coordinate - wall_lo) > tol or abs(top.coordinate - wall_hi) > tol
        ):
            continue
        flat_centres = tuple(
            (
                _rounded(float(point.X), 3),
                _rounded(float(point.Y), 3),
                _rounded(float(point.Z), 3),
            )
            for point in centres
        )
        flat_directions = tuple(
            _flat_direction(headings[index], transverse_indices) for index in ordered
        )
        center = [0.0, 0.0, 0.0]
        center[transverse_indices[0]] = cx
        center[transverse_indices[1]] = cy
        center[axis_index] = (base.coordinate + top.coordinate) / 2
        record_type = PolygonalStock if whole_stock else PolygonalBoss
        record_center = (center[0], center[1], center[2])
        record = record_type(
            axis=axis,
            center=(
                _rounded(record_center[0], 4),
                _rounded(record_center[1], 4),
                _rounded(record_center[2], 4),
            ),
            side_count=side_count,
            across_flats=_rounded(across, 4),
            base=_rounded(base.coordinate, 4),
            top=_rounded(top.coordinate, 4),
            flat_directions=flat_directions,
            flat_centres=flat_centres,
        )
        found.append(
            _PolygonalProposal(
                record,
                tuple(graph.face(node) for node in ordered),
                graph.face(base.node),
                graph.face(top.node),
                None if whole_stock else graph.face(top.node if positive else base.node),
            )
        )
    return found


def recognise_polygonal_bosses(
    part: Part,
    *,
    tol: float | None = None,
    angle_tol: float = math.radians(2),
    graph: GeometryGraph | None = None,
) -> list[PolygonalBoss]:
    """Return regular hexagonal principal-axis bosses independently per physical solid.

    A candidate is accepted from a closed ring of outward planar side faces, opposed
    support planes with one A/F value, and common attached support/top caps. A whole prism,
    a blind recess, or faces assembled across separate solids cannot satisfy that evidence.

    *graph* is an existing graph over *part*, from a caller running several recognisers over
    one solid. It is used only when *part* is a single solid: with more than one, this looks at
    each solid separately on purpose -- a ring assembled from faces of two solids is not a boss
    -- and a whole-part graph would be the wrong inventory to ask.
    """
    return _discover_polygonal_bosses(part, tol=tol, angle_tol=angle_tol, graph=graph)


def _discover_polygonal_bosses(
    part: Part,
    *,
    tol: float | None = None,
    angle_tol: float = math.radians(2),
    graph: GeometryGraph | None = None,
    writer: object | None = None,
) -> list[PolygonalBoss]:
    """Shared Polygonal Boss discovery with optional aggregate evidence issuance."""

    solids = list(part.solids())
    sources = solids if len(solids) > 1 else [part]
    shared = graph if len(sources) == 1 else None
    proposals: list[_PolygonalProposal] = []
    for solid in sources:
        owner = shared if shared is not None else GeometryGraph(solid)
        for axis in ("z", "x", "y"):
            proposals.extend(
                proposal
                for proposal in _recognise_one(
                    solid,
                    tol=tol,
                    angle_tol=angle_tol,
                    axis=axis,
                    graph=owner,
                )
                if isinstance(proposal.record, PolygonalBoss)
            )
    proposals.sort(key=lambda proposal: proposal.record)
    records = [cast("PolygonalBoss", proposal.record) for proposal in proposals]
    if writer is None:
        return records
    bridge = GeometryEvidenceBridge(writer, shared)

    pending: list[tuple[PolygonalBoss, tuple[FaceRef, ...], tuple[FaceRef, ...]]] = []
    used: set[FaceRef] = set()
    for proposal, record in zip(proposals, records, strict=True):
        refs = bridge.refs(proposal.side_faces)
        resolved = set(refs)
        if len(refs) != 6:
            raise ValueError("a Polygonal Boss requires six distinct original side faces")
        if used & resolved:
            raise ValueError("Polygonal Boss occurrences share defining side faces")
        used.update(resolved)
        bridge.validate_defining(refs)
        if proposal.terminal_cap is None:
            raise ValueError("a Polygonal Boss requires one retained terminal cap")
        terminal = bridge.refs((proposal.terminal_cap,))
        if len(terminal) != 1 or terminal[0] in resolved:
            raise ValueError("Polygonal Boss terminal cap identity is unavailable")
        constituent: tuple[FaceRef, ...] = (*refs, *terminal)
        bridge.validate_defining(constituent)
        pending.append((record, refs, constituent))
    for record, refs, constituent in pending:
        bridge.add_defining(
            record,
            refs,
            family=FamilyId.POLYGONAL_BOSSES,
            constituent=constituent,
        )
    return records


def recognise_polygonal_stock(
    part: Part,
    *,
    tol: float | None = None,
    angle_tol: float = math.radians(2),
    graph: GeometryGraph | None = None,
) -> list[PolygonalStock]:
    """Return one record only when the complete part is a regular hexagonal prism.

    The exact-prism boundary is fail closed: multi-solid assemblies and solids with any
    additional or missing face are not silently promoted to stock.

    *graph* is an existing graph over *part*, as above. This one asks about the whole part and
    only ever with a single solid, so there is no case where the caller's graph is the wrong
    inventory.
    """
    return _discover_polygonal_stock(
        part,
        tol=tol,
        angle_tol=angle_tol,
        graph=graph,
    )


def _discover_polygonal_stock(
    part: Part,
    *,
    tol: float | None = None,
    angle_tol: float = math.radians(2),
    graph: GeometryGraph | None = None,
    writer: object | None = None,
) -> list[PolygonalStock]:
    """Discover exact-prism stock and optionally issue its complete eight-face boundary."""

    if len(list(part.solids())) != 1 or len(list(part.faces())) != 8:
        return []
    bridge = GeometryEvidenceBridge(writer, graph) if writer is not None else None
    owner = bridge.geometry if bridge is not None else graph
    proposals: list[_PolygonalProposal] = []
    # Z first preserves the established direct-path work and byte-level record behavior. An
    # exact eight-face prism can establish only one cap axis; stop at the first supported axis
    # rather than scanning unrelated orientations after the complete boundary is proved.
    for axis in ("z", "x", "y"):
        proposals = [
            proposal
            for proposal in _recognise_one(
                part,
                tol=tol,
                angle_tol=angle_tol,
                whole_stock=True,
                axis=axis,
                graph=owner,
            )
            if isinstance(proposal.record, PolygonalStock)
        ]
        if proposals:
            break
    proposals.sort(key=lambda proposal: proposal.record)
    records = [cast("PolygonalStock", proposal.record) for proposal in proposals]
    if writer is None:
        return records

    assert bridge is not None
    pending: list[tuple[PolygonalStock, tuple[FaceRef, ...]]] = []
    for proposal, record in zip(proposals, records, strict=True):
        refs = bridge.refs((*proposal.side_faces, proposal.lower_cap, proposal.upper_cap))
        if len(refs) != 8 or set(refs) != set(bridge.geometry.faces):
            raise ValueError("Polygonal Stock requires its complete eight-face graph inventory")
        bridge.validate_defining(refs)
        pending.append((record, refs))
    for record, refs in pending:
        bridge.add_defining(record, refs, family=FamilyId.POLYGONAL_STOCK)
    return records
