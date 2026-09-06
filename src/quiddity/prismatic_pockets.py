# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Pockets of any prismatic cross-section, found from wall rings or bounded cavity regions.

A **prismatic pocket** is a closed ring of planar walls with a floor filling one end and the
other end open: the capped sibling of :class:`quiddity.Passage`, and the same feature
MFCAD++ labels *Triangular pocket*, *6-sided pocket* and *Rectangular pocket*.

**Why a second pocket family rather than an extension of the first.**
:func:`quiddity.recognise_pockets` finds a recess by sorting walls into buckets by the
axis their normal aligns with and pairing walls within a bucket. A triangular recess has no two
walls sharing an axis, so no pair forms and no gate ever runs -- measured over 600 MFCAD++
models, **94% of triangular-pocket faces never reach a test**, which is why that family scores 0%
on them and 4% on hexagonal ones. Nothing about the geometry is hard; the search cannot see it.

Ring-walking has no such blind spot, and this is not a hope: `recognise_passages` already walks
these exact rings and scores 59% on triangular passages, on the same solids where pairing scores
0% on triangular pockets. It also already *finds* these pockets and discards them -- the line
that did so read ``continue  # a floor fills the ring: this is a pocket``.

**What each family is for.** They are not redundant, and neither is going away:

- this one reaches any planar cross-section, and is the only path to a non-rectangular recess;
- :func:`quiddity.recognise_pockets` reaches recesses this cannot -- an obround pocket's
  ends are cylindrical, so its walls form no closed *planar* ring at all. Measured: **zero** rings
  on the *Circular end pocket* class, which the pairing family recognises.

Where both see the same rectangular recess, both report it. That overlap is a reconciliation
question and is answered by
:func:`quiddity._reconcile.prismatic_pockets_that_are_not_pockets`, from the faces
each family claimed -- not by either recogniser declining a face because the other might want
it, which ADR 0003 forbids.

Ring-finding, the cross-section walk and the cap test are
:mod:`quiddity._rings`, shared with ``passages``.

A partial chamfer or rolling treatment can shorten one wall and break the equal-span ring while
leaving the physical pocket unambiguous. The fallback starts at that one exterior inner wire,
walks only concave/smooth cavity incidences, reconstructs the polygon from the direct planar wall
cycle behind the treatment, and proves the complete empty section plus one material-backed floor.
It is deliberately local to this family rather than a public or post-recognition cavity walk.
"""

from __future__ import annotations

import math
from collections import defaultdict
from dataclasses import dataclass

from build123d import Face, Solid, Vector, Wire

from quiddity._adjacency import FaceEdges, FaceGraph, FaceNode
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._geometry import AXIS_ZERO_COS
from quiddity._record import Record
from quiddity._rings import (
    SPAN_EPS,
    _canonical,
    _capped_ends,
    _centroid,
    _cross_section,
    _is_void,
    rings,
)
from quiddity._typing import Part
from quiddity._volume_probe import material_fraction as _material_fraction
from quiddity._wire_seed import wire_seed as _wire_seed


@dataclass(frozen=True, order=True)
class PrismaticPocket(Record):
    """A floored recess of constant planar cross-section, open at one end.

    ``axis`` is the direction the walls run ("x"/"y"/"z"); ``sides`` is the number of walls, so
    a triangular pocket reports 3 and a hexagonal one 6; ``depth`` is how far it runs from the
    open end down to the floor; ``at`` is the centre of the void in part space; ``open_sign`` is
    ``+1`` when the opening is at the high end of ``axis`` and ``-1`` at the low end.

    ``section`` is the cross-section: its corners in part coordinates, in the two axes other
    than ``axis`` and in that axis order, walked around the ring. It is carried for the reason
    :class:`quiddity.Passage` carries one -- without it a triangular pocket and a
    hexagonal one of the same depth differ only in ``sides``, and the shape a machinist needs to
    see is gone.

    **Not a :class:`quiddity.Pocket`, deliberately.** That record measures ``width`` and
    ``length`` on two named axes, which is a true statement about a rectangular recess and a
    meaningless one about a triangle. Folding these in would have made ``Pocket.width`` sometimes
    a wall-to-wall measurement and sometimes a bounding-box extent, with no way for a caller
    reading it to tell -- a change of meaning for every existing consumer, including those that
    only ever see rectangular pockets. Two sibling records under one field cost a consumer
    nothing it is not already paying.

    The corner walk is canonical rather than the kernel's, as ``Passage``'s is: equivalent
    geometry gives an equal record however the part was traversed.
    """

    axis: str
    sides: int
    depth: float
    open_sign: int
    at: tuple[float, float, float]
    section: tuple[tuple[float, float], ...]


@dataclass(frozen=True, slots=True)
class _RecoveredPocket:
    """One interrupted wall ring proved from an exterior mouth to one floor."""

    axis: int
    low: float
    high: float
    open_sign: int
    section: tuple[tuple[float, float], ...]
    walls: tuple[FaceNode, ...]
    constituent: frozenset[FaceNode]


_END_PROBE = 2e-5
_MATERIAL_VOL_FRAC = 1e-9


def _section_prism(
    section: tuple[tuple[float, float], ...], axis: int, low: float, high: float
) -> Solid:
    if high - low <= 2 * SPAN_EPS:
        raise ValueError("pocket probe prism is too short")
    others = [other for other in range(3) if other != axis]

    def point(at: float, across: tuple[float, float]) -> Vector:
        values = [0.0, 0.0, 0.0]
        values[axis] = at
        values[others[0]], values[others[1]] = across
        return Vector(*values)

    points = tuple(point(low + SPAN_EPS, corner) for corner in section)
    wire = Wire.make_polygon((*points, points[0]))
    vector = [0.0, 0.0, 0.0]
    vector[axis] = high - low - 2 * SPAN_EPS
    return Solid.extrude(Face(wire), Vector(*vector))


def _section_slab(
    section: tuple[tuple[float, float], ...],
    axis: int,
    at: float,
    sign: int,
    thickness: float,
) -> Solid:
    low, high = sorted((at + sign * SPAN_EPS, at + sign * thickness))
    return _section_prism(section, axis, low - SPAN_EPS, high + SPAN_EPS)




def _void_open_and_floored(
    part: Part,
    section: tuple[tuple[float, float], ...],
    axis: int,
    mouth_at: float,
    floor_at: float,
) -> bool:
    low, high = sorted((mouth_at, floor_at))
    centre = _centroid(section)
    radius = max(math.dist(point, centre) for point in section)
    thickness = max(_END_PROBE, (high - low) * 1e-4, radius * 1e-4)
    mouth_sign = 1 if mouth_at == high else -1
    try:
        interior = _section_prism(section, axis, low, high)
        mouth = _section_slab(section, axis, mouth_at, mouth_sign, thickness)
        floor = _section_slab(section, axis, floor_at, -mouth_sign, thickness)
        return (
            _material_fraction(part, interior) <= _MATERIAL_VOL_FRAC
            and _material_fraction(part, mouth) <= _MATERIAL_VOL_FRAC
            and _material_fraction(part, floor) >= 1.0 - _MATERIAL_VOL_FRAC
        )
    except (RuntimeError, TypeError, ValueError, ZeroDivisionError):
        return False




def _inner_region(
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
            if kind in ("concave", "smooth"):
                region.add(neighbour)
                pending.append(neighbour)
    return frozenset(region)


def _axis_for_opening(graph: FaceGraph, opening: FaceNode) -> int | None:
    normal = graph.normal(opening)
    if normal is None:
        return None
    axes = [axis for axis in range(3) if abs(normal[axis]) >= 1.0 - AXIS_ZERO_COS]
    return axes[0] if len(axes) == 1 else None


def _plane_at(graph: FaceGraph, node: FaceNode, axis: int) -> float | None:
    normal = graph.normal(node)
    if normal is None or abs(normal[axis]) < 1.0 - AXIS_ZERO_COS:
        return None
    low, high = graph.bounds(node)[axis]
    return 0.5 * (low + high) if high - low <= SPAN_EPS else None


def _floor_section(wire: Wire, axis: int) -> tuple[tuple[float, float], ...] | None:
    """Read one straight-edged floor boundary in exact wire order."""

    try:
        edges = tuple(wire.edges())
        if len(edges) < 3 or any(edge.geom_type.name != "LINE" for edge in edges):
            return None
        others = [other for other in range(3) if other != axis]
        corners = []
        for index, edge in enumerate(edges):
            shared = tuple(
                left
                for left in edges[index - 1].vertices()
                for right in edge.vertices()
                if left == right
            )
            if len(shared) != 1:
                return None
            point = shared[0]
            values = (float(point.X), float(point.Y), float(point.Z))
            corners.append((values[others[0]], values[others[1]]))
        return _canonical(corners)
    except (AttributeError, RuntimeError, TypeError, ValueError):
        return None


def _floor_seeded_regions(part: Part, graph: FaceGraph) -> tuple[_RecoveredPocket, ...]:
    """Recover a prismatic Pocket whose intact floor outlives a corrupted mouth-side cycle.

    The floor boundary is recognition evidence, not a label-derived reconstruction. Every edge
    must meet one concave planar wall support on one principal run, and the resulting complete
    section must remain void through one exterior termination with material immediately behind
    the floor. Extra side openings may interrupt wall adjacency but cannot change that proof.
    """

    recovered = []
    for floor in graph.nodes:
        if not graph.is_planar(floor):
            continue
        normal = graph.normal(floor)
        if normal is None:
            continue
        axes = [axis for axis in range(3) if abs(normal[axis]) >= 1.0 - AXIS_ZERO_COS]
        if len(axes) != 1:
            continue
        axis = axes[0]
        wires = tuple(graph.face(floor).wires())
        if len(wires) != 1:
            continue
        wire = wires[0]
        section = _floor_section(wire, axis)
        # Once its mouth-side cycle is broken, a four-sided floor cannot by itself distinguish
        # an interrupted Pocket from a rectangular blind-slot run.  Those shapes retain their
        # more specific producers; this fallback is the non-rectangular proof they cannot make.
        if section is None or len(section) == 4:
            continue
        walls = tuple(sorted(_wire_seed(graph, floor, wire), key=lambda node: node.index))
        floor_arcs = {}
        for node in walls:
            floor_arcs[node] = graph.arc(floor, node)
        if len(walls) != len(section) or any(
            not graph.is_planar(node)
            or floor_arcs[node] != "concave"
            or (wall_normal := graph.normal(node)) is None
            or abs(wall_normal[axis]) > AXIS_ZERO_COS
            for node in walls
        ):
            continue
        floor_at = _plane_at(graph, floor, axis)
        if floor_at is None:
            continue
        far = []
        for wall in walls:
            low, high = graph.bounds(wall)[axis]
            if abs(low - floor_at) <= SPAN_EPS:
                far.append(high)
            elif abs(high - floor_at) <= SPAN_EPS:
                far.append(low)
            else:
                break
        else:
            directions = {1 if value > floor_at else -1 for value in far}
            if len(directions) != 1:
                continue
            direction = directions.pop()
            mouth_at = max(far) if direction > 0 else min(far)
            if abs(mouth_at - floor_at) <= SPAN_EPS:
                continue
            owner = graph.common_valid_solid((*walls, floor))
            low, high = sorted((mouth_at, floor_at))
            caps = _capped_ends(graph, walls, set(walls), axis, low, high)
            floor_index = 0 if floor_at == low else 1
            mouth_index = 1 - floor_index
            if (
                caps[floor_index] != frozenset({floor})
                or caps[mouth_index]
                or owner is None
                or not _void_open_and_floored(
                graph.solid_shape(owner), section, axis, mouth_at, floor_at
                )
            ):
                continue
            recovered.append(
                _RecoveredPocket(
                    axis,
                    low,
                    high,
                    direction,
                    section,
                    walls,
                    frozenset((*walls, floor)),
                )
            )
    recovered.sort(key=lambda item: (item.axis, _centroid(item.section), item.low, item.high))
    return tuple(recovered)


def _one_ended_regions(part: Part, graph: FaceGraph) -> tuple[_RecoveredPocket, ...]:
    """Recover unique principal-axis polygonal cavities whose wall spans are interrupted."""

    raw: dict[
        frozenset[FaceNode], list[tuple[FaceNode, frozenset[FaceNode], int]]
    ] = defaultdict(list)
    for opening in graph.nodes:
        axis = _axis_for_opening(graph, opening) if graph.is_planar(opening) else None
        if axis is None:
            continue
        for wire in graph.face(opening).inner_wires():
            seed = _wire_seed(graph, opening, wire)
            mouth_arc_list = []
            for node in seed:
                kind = graph.arc(opening, node)
                mouth_arc_list.append(kind)
            mouth_arcs = tuple(mouth_arc_list)
            if (
                not seed
                or not all(kind in ("convex", "smooth") for kind in mouth_arcs)
                or "convex" not in mouth_arcs
            ):
                continue
            raw[_inner_region(graph, opening, seed)].append((opening, seed, axis))

    intersecting = {
        region
        for region in raw
        if any(region != other and region & other for other in raw)
    }
    recovered = []
    for region, mouths in raw.items():
        if region in intersecting or len(mouths) != 1:
            continue
        opening, seed, axis = mouths[0]
        mouth_at = _plane_at(graph, opening, axis)
        floor_planes = []
        for node in region:
            at = _plane_at(graph, node, axis)
            if at is not None and mouth_at is not None and abs(at - mouth_at) > SPAN_EPS:
                floor_planes.append((at, node))
        floor_groups: list[tuple[float, set[FaceNode]]] = []
        for at, node in sorted(floor_planes, key=lambda item: (item[0], item[1].index)):
            if floor_groups and abs(at - floor_groups[-1][0]) <= SPAN_EPS:
                floor_groups[-1][1].add(node)
            else:
                floor_groups.append((at, {node}))
        if mouth_at is None or len(floor_groups) != 1:
            continue
        floor_at, floor_nodes = floor_groups[0]
        walls = tuple(
            sorted(
                (
                    node
                    for node in region
                    if graph.is_planar(node)
                    and (normal := graph.normal(node)) is not None
                    and abs(normal[axis]) <= AXIS_ZERO_COS
                ),
                key=lambda node: node.index,
            )
        )
        wall_set = set(walls)
        interruptions = region - wall_set - floor_nodes
        if (
            not interruptions <= seed
            or len(walls) < 3
            or any(len(set(graph.neighbours(node)) & wall_set) != 2 for node in walls)
        ):
            continue
        section = _cross_section(graph, walls, wall_set, axis)
        low, high = sorted((mouth_at, floor_at))
        solid = graph.common_valid_solid(region | {opening})
        if (
            section is None
            or high - low <= SPAN_EPS
            or solid is None
            or not _is_void(graph.solid_shape(solid), section, axis, low, high)
            or not _void_open_and_floored(
                graph.solid_shape(solid), section, axis, mouth_at, floor_at
            )
        ):
            continue
        recovered.append(
            _RecoveredPocket(
                axis,
                low,
                high,
                1 if mouth_at > floor_at else -1,
                section,
                walls,
                region,
            )
        )
    recovered.sort(key=lambda item: (item.axis, _centroid(item.section), item.low, item.high))
    return tuple(recovered)


def recognise_prismatic_pockets(
    part: Part,
    *,
    face_edges: FaceEdges | None = None,
    ledger: ClaimLedger | EvidenceWriter | None = None,
) -> list[PrismaticPocket]:
    """Recognise the prismatic pockets of *part* (see module docstring).

    Returns one :class:`PrismaticPocket` per ring capped at exactly one end, or per uniquely
    bounded interrupted ring recovered from one exterior mouth, sorted deterministically. A void
    capped at *both* ends is an enclosed cavity and is not reported by any family here: it is not
    reachable by a tool, so it is not a machined recess.

    **A rectangular recess is reported here as well as by**
    :func:`quiddity.recognise_pockets`, because on the ring alone it is one. Which
    survives is
    :func:`quiddity._reconcile.prismatic_pockets_that_are_not_pockets`, decided from
    the claims rather than by either family second-guessing the other.

    *ledger* records the faces the pocket was **established by**: its planar wall supports. The
    exact cap and interruption faces selected by that proof are wider constituent evidence, not
    defining claims. The floor makes the recess blind; on an interrupted ring, its unique plane
    and the exterior mouth also bound the physical depth without becoming ownership evidence.
    This preserves the same ownership line
    :func:`quiddity.recognise_pockets` draws for the recess it finds by pairing.
    """

    graph = FaceGraph(part, face_edges=face_edges) if ledger is None else ledger.graph
    found: list[tuple[PrismaticPocket, tuple[FaceNode, ...], frozenset[FaceNode]]] = []
    for ring in rings(part, graph):
        low_capped, high_capped = ring.caps
        if low_capped == high_capped:
            continue  # neither: a passage. Both: an enclosed cavity, which no tool reaches.
        axis, section = ring.axis, ring.section
        others = [a for a in (0, 1, 2) if a != axis]
        middle = _centroid(section)
        at = [0.0, 0.0, 0.0]
        at[axis] = 0.5 * (ring.low + ring.high)
        at[others[0]], at[others[1]] = middle
        found.append(
            (
                PrismaticPocket(
                    axis="xyz"[axis],
                    sides=len(ring.nodes),
                    depth=round(ring.high - ring.low, 3),
                    # The floor caps one end, so the opening is the other.
                    open_sign=1 if low_capped else -1,
                    at=(round(at[0], 3), round(at[1], 3), round(at[2], 3)),
                    section=tuple((round(u, 3), round(v, 3)) for u, v in section),
                ),
                tuple(ring.nodes),
                frozenset(ring.nodes) | ring.cap_nodes[0] | ring.cap_nodes[1],
            )
        )

    existing_walls = {frozenset(nodes) for _record, nodes, _constituent in found}
    for recovered in (*_one_ended_regions(part, graph), *_floor_seeded_regions(part, graph)):
        wall_set = frozenset(recovered.walls)
        if wall_set in existing_walls:
            continue
        existing_walls.add(wall_set)
        others = [axis for axis in range(3) if axis != recovered.axis]
        centre = _centroid(recovered.section)
        at = [0.0, 0.0, 0.0]
        at[recovered.axis] = 0.5 * (recovered.low + recovered.high)
        at[others[0]], at[others[1]] = centre
        found.append(
            (
                PrismaticPocket(
                    axis="xyz"[recovered.axis],
                    sides=len(recovered.walls),
                    depth=round(recovered.high - recovered.low, 3),
                    open_sign=recovered.open_sign,
                    at=(round(at[0], 3), round(at[1], 3), round(at[2], 3)),
                    section=tuple(
                        (round(u, 3), round(v, 3)) for u, v in recovered.section
                    ),
                ),
                recovered.walls,
                recovered.constituent,
            )
        )

    found.sort(key=lambda item: (item[0].axis, item[0].at, item[0].section))
    if ledger is not None:
        writer = ledger.writer if isinstance(ledger, ClaimLedger) else ledger
        for pocket, nodes, constituent in found:
            writer.add_defining(
                pocket,
                nodes,
                family=FamilyId.PRISMATIC_POCKETS,
                constituent=constituent,
            )
    return [pocket for pocket, _nodes, _constituent in found]
