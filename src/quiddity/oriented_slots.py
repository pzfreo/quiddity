# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Rectangular through slots whose in-plane axes are not principal in the supplied frame."""

from __future__ import annotations

import math
from collections import Counter
from collections.abc import Sequence
from dataclasses import dataclass

from quiddity._adjacency import FaceGraph, FaceNode, SolidRef
from quiddity._candidates import EvidenceSink, FamilyId
from quiddity._claims import ClaimLedger, EvidenceWriter
from quiddity._geometry import AXIS_ALIGNED_COS, body_signature
from quiddity._pattern_geometry import _linear_array_candidates, _plane_uv, _rect_grid
from quiddity._record import Record
from quiddity._section_passages import SectionRingProposal, section_ring_proposals
from quiddity._typing import Part, Vector3
from quiddity.passages import SectionPassage, _section_passage_record

_body_signature = body_signature

# OrientedSlot dimensions are serialized to 0.001 model units. These established projection bounds
# remain conservative after PassageSection schema v2 increases its point precision to 0.0001; they
# are not a feature-size tolerance and never admit a section without the exact neutral ring proof.
_SERIALIZATION_QUANTUM = 1e-3
_VECTOR_ERROR = 4.0 * _SERIALIZATION_QUANTUM


def _dot(left: tuple[float, ...], right: tuple[float, ...]) -> float:
    return sum(a * b for a, b in zip(left, right, strict=True))


def _length(vector: tuple[float, ...]) -> float:
    return math.sqrt(_dot(vector, vector))


def _canonical_direction(vector: Vector3) -> Vector3:
    norm = _length(vector)
    if norm <= 0.0:
        raise ValueError("slot direction must be nonzero")
    result = tuple(component / norm for component in vector)
    pivot = max(range(3), key=lambda axis: (abs(result[axis]), axis))
    if result[pivot] < 0.0:
        result = tuple(-component for component in result)
    return tuple(0.0 if abs(component) < 5e-13 else component for component in result)  # type: ignore[return-value]


def _world_direction(source: SectionPassage, vector: tuple[float, float]) -> Vector3:
    return _canonical_direction(
        tuple(
            vector[0] * source.frame.u[axis] + vector[1] * source.frame.v[axis]
            for axis in range(3)
        )  # type: ignore[arg-type]
    )


def _principal(direction: Vector3) -> bool:
    return max(abs(component) for component in direction) >= AXIS_ALIGNED_COS


@dataclass(frozen=True, order=True, slots=True)
class OrientedSlot(Record):
    """A rectangular through slot with free in-plane width and long directions.

    ``source`` is the exact accepted rectangular :class:`SectionPassage` occurrence from which
    this physical interpretation was issued.  Its frame and run interval carry the through
    direction/span. ``body_key`` has the same fail-closed compound meaning as legacy ``Slot``.
    """

    source: SectionPassage
    width_direction: Vector3
    long_direction: Vector3
    width: float
    length: float
    center: Vector3
    body_key: tuple[float, ...] | None = ()

    @property
    def depth_direction(self) -> Vector3:
        return self.source.frame.run

    @property
    def depth(self) -> float:
        return self.source.run_interval[1] - self.source.run_interval[0]

    @property
    def location(self) -> Vector3:
        return self.center


@dataclass(frozen=True, order=True, slots=True)
class OrientedSlotArray(Record):
    slots: tuple[OrientedSlot, ...]
    pitch: float
    direction: Vector3


@dataclass(frozen=True, order=True, slots=True)
class OrientedSlotGrid(Record):
    slots: tuple[OrientedSlot, ...]
    rows: int
    cols: int
    row_pitch: float
    col_pitch: float
    angle: float
    center: Vector3


def _rectangle(source: SectionPassage) -> tuple[Vector3, Vector3, float, float] | None:
    boundary = source.section.boundary
    if len(boundary) != 4 or any(vertex.bulge != 0.0 for vertex in boundary):
        return None
    points = tuple(vertex.point for vertex in boundary)
    edges = tuple(
        tuple(points[(at + 1) % 4][axis] - point[axis] for axis in range(2))
        for at, point in enumerate(points)
    )
    lengths = tuple(_length(edge) for edge in edges)
    if min(lengths) <= 2.0 * _VECTOR_ERROR:
        return None
    if any(abs(lengths[at] - lengths[at + 2]) > _VECTOR_ERROR for at in (0, 1)):
        return None
    if any(
        _length(tuple(edges[at][axis] + edges[at + 2][axis] for axis in range(2)))
        > _VECTOR_ERROR
        for at in (0, 1)
    ):
        return None
    orthogonal_error = _VECTOR_ERROR / min(lengths[0], lengths[1])
    if abs(_dot(edges[0], edges[1]) / (lengths[0] * lengths[1])) > orthogonal_error:
        return None
    if abs(lengths[0] - lengths[1]) <= _VECTOR_ERROR:
        return None
    long_at = 0 if lengths[0] > lengths[1] else 1
    width_at = 1 - long_at
    long_direction = _world_direction(
        source, (edges[long_at][0], edges[long_at][1])
    )
    width_direction = _world_direction(
        source, (edges[width_at][0], edges[width_at][1])
    )
    return width_direction, long_direction, lengths[width_at], lengths[long_at]


def _project(source: SectionPassage, body_key: tuple[float, ...] | None) -> OrientedSlot | None:
    rectangle = _rectangle(source)
    if rectangle is None:
        return None
    width_direction, long_direction, width, length = rectangle
    # Principal sections belong to the legacy axis-letter Slot family.  This successor exists
    # only where that schema cannot represent the physical directions.
    if _principal(width_direction) and _principal(long_direction):
        return None
    run_midpoint = 0.5 * sum(source.run_interval)
    center = tuple(
        round(source.frame.origin[axis] + run_midpoint * source.frame.run[axis], 3)
        for axis in range(3)
    )
    return OrientedSlot(
        source=source,
        width_direction=tuple(round(value, 6) for value in width_direction),  # type: ignore[arg-type]
        long_direction=tuple(round(value, 6) for value in long_direction),  # type: ignore[arg-type]
        width=round(width, 3),
        length=round(length, 3),
        center=center,  # type: ignore[arg-type]
        body_key=body_key,
    )


def _body_keys(
    graph: FaceGraph, solids: tuple[SolidRef, ...]
) -> dict[SolidRef, tuple[float, ...] | None]:
    # Ambiguity is a property of the complete input, not just bodies on which this
    # detector happened to find a slot. A non-slot body can have the same signature.
    unique = tuple(
        dict.fromkeys(
            solid
            for node in graph.nodes
            if (solid := graph.common_valid_solid((node,))) is not None
        )
    )
    signatures = {solid: body_signature(graph.solid_shape(solid)) for solid in unique}
    counts = Counter(signatures.values())
    return {
        solid: signatures[solid] if counts[signatures[solid]] == 1 else None
        for solid in dict.fromkeys(solids)
    }


def _from_proposals(
    graph: FaceGraph,
    proposals: tuple[SectionRingProposal, ...],
    sink: EvidenceSink | None,
) -> list[OrientedSlot]:
    keys = _body_keys(graph, tuple(proposal.solid for proposal in proposals))
    found: list[tuple[OrientedSlot, tuple[FaceNode, ...]]] = []
    for proposal in proposals:
        record = _project(_section_passage_record(proposal), keys[proposal.solid])
        if record is not None:
            found.append((record, proposal.nodes))
    found.sort(key=lambda item: item[0])
    if sink is not None:
        for record, nodes in found:
            sink.propose(FamilyId.ORIENTED_SLOTS, record, defining=nodes)
    return [record for record, _nodes in found]


def recognise_oriented_slots(
    part: Part, *, ledger: ClaimLedger | EvidenceWriter | None = None
) -> list[OrientedSlot]:
    """Recognise rectangular through slots with non-principal in-plane directions."""

    graph = FaceGraph(part) if ledger is None else ledger.graph
    return _from_proposals(
        graph,
        section_ring_proposals(part, graph),
        None if ledger is None else ledger.sink,
    )


def _pattern_key(slot: OrientedSlot) -> tuple[object, ...]:
    depth_plane = round(_dot(slot.center, slot.source.frame.run), 3)
    return (
        slot.width_direction,
        slot.long_direction,
        round(slot.width, 3),
        round(slot.length, 3),
        slot.source.frame.run,
        slot.source.run_interval,
        depth_plane,
        slot.body_key,
    )


def _linear(members, pitch, direction) -> OrientedSlotArray:
    return OrientedSlotArray(tuple(members), pitch, direction)


def _grid(members, rows, cols, row_pitch, col_pitch, angle, center) -> OrientedSlotGrid:
    return OrientedSlotGrid(tuple(members), rows, cols, row_pitch, col_pitch, angle, center)


def recognise_oriented_slot_patterns(
    slots: Sequence[OrientedSlot],
) -> list[OrientedSlotArray | OrientedSlotGrid]:
    """Derive coplanar, same-body arrays/grids of geometrically identical oriented slots."""

    groups: dict[tuple[object, ...], list[OrientedSlot]] = {}
    for slot in slots:
        if slot.body_key is not None:
            groups.setdefault(_pattern_key(slot), []).append(slot)
    patterns: list[OrientedSlotArray | OrientedSlotGrid] = []
    for members in groups.values():
        if len(members) < 3:
            continue
        u, v = _plane_uv(members[0].depth_direction)
        points = [
            (
                sum(a * b for a, b in zip(slot.center, u, strict=True)),
                sum(a * b for a, b in zip(slot.center, v, strict=True)),
            )
            for slot in members
        ]
        candidates: list = []
        grid = _rect_grid(members, points, _grid)
        if grid is not None:
            candidates.append((grid, frozenset(range(len(members)))))
        candidates.extend(_linear_array_candidates(members, points, _linear))
        candidates.sort(key=lambda candidate: -len(candidate[1]))
        used: set[int] = set()
        for pattern, indices in candidates:
            if not indices & used:
                patterns.append(pattern)
                used |= indices
    return patterns
