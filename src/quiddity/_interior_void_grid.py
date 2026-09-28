# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Bounded six-axis air evidence for the interior-void recogniser.

Each grid line intersects the original B-rep, not a tessellation.  A cell is
material only when all three line sweeps agree; it is enclosed air only when
all three agree it is air and material lies on both sides on every axis.
Lines with odd, coincident or failed intersections give no evidence.  The grid
is a sampling instrument, never a substitute solid or an exact volume proof.
"""

from __future__ import annotations

import math
from bisect import bisect_right
from collections import deque
from dataclasses import dataclass
from heapq import heappop, heappush

from OCP.gp import gp_Dir, gp_Lin, gp_Pnt
from OCP.IntCurvesFace import IntCurvesFace_ShapeIntersector
from OCP.Standard import Standard_Failure, Standard_NullObject

from quiddity._geometry import COORD_FLOOR, length_tol
from quiddity._typing import Part

_CELLS_ON_LONGEST_AXIS = 64
_MIN_COMPONENT_SPAN = 3  # fewer cells cannot establish a three-dimensional region

Cell = tuple[int, int, int]


@dataclass(frozen=True, slots=True)
class VoidGrid:
    origin: tuple[float, float, float]
    pitch: float
    dimensions: Cell
    material: frozenset[Cell]
    air: frozenset[Cell]
    components: tuple[frozenset[Cell], ...]
    air_samples: int

    def center(self, cell: Cell) -> tuple[float, float, float]:
        return (
            self.origin[0] + (cell[0] + 0.5) * self.pitch,
            self.origin[1] + (cell[1] + 0.5) * self.pitch,
            self.origin[2] + (cell[2] + 0.5) * self.pitch,
        )


def _neighbours(cell: Cell) -> tuple[Cell, ...]:
    x, y, z = cell
    return (
        (x - 1, y, z),
        (x + 1, y, z),
        (x, y - 1, z),
        (x, y + 1, z),
        (x, y, z - 1),
        (x, y, z + 1),
    )


def expanded_air_components(grid: VoidGrid) -> frozenset[int]:
    """Find cores wider than every sampled route from exterior air.

    Clearance is a conservative six-neighbour grid distance to material.  A
    widest-path flood carries the largest possible throat clearance from the
    bounding-box boundary.  This rejects constant-width blind pockets, even
    when their axis is oblique to the sampling frame.
    """

    clearance: dict[Cell, int] = {}
    pending: deque[Cell] = deque()
    for cell in grid.air:
        if any(neighbour in grid.material for neighbour in _neighbours(cell)):
            clearance[cell] = 1
            pending.append(cell)
    while pending:
        cell = pending.popleft()
        distance = clearance[cell] + 1
        for neighbour in _neighbours(cell):
            if neighbour in grid.air and neighbour not in clearance:
                clearance[neighbour] = distance
                pending.append(neighbour)

    nx, ny, nz = grid.dimensions
    widest: dict[Cell, int] = {}
    frontier: list[tuple[int, Cell]] = []
    infinity = max(grid.dimensions) + 1
    for cell in grid.air:
        if 0 in cell or cell[0] == nx - 1 or cell[1] == ny - 1 or cell[2] == nz - 1:
            widest[cell] = infinity
            heappush(frontier, (-infinity, cell))
    while frontier:
        negative_width, cell = heappop(frontier)
        width = -negative_width
        if width < widest[cell]:
            continue
        for neighbour in _neighbours(cell):
            if neighbour not in grid.air or neighbour not in clearance:
                continue
            next_width = min(width, clearance[neighbour])
            if next_width > widest.get(neighbour, 0):
                widest[neighbour] = next_width
                heappush(frontier, (-next_width, neighbour))

    expanded = set()
    for index, component in enumerate(grid.components):
        core_width = max(clearance.get(cell, 0) for cell in component)
        throat_width = max(widest.get(cell, 0) for cell in component)
        if core_width > 0 and (throat_width == 0 or core_width >= 1.5 * throat_width):
            expanded.add(index)
    return frozenset(expanded)


def _components(cells: set[Cell]) -> tuple[frozenset[Cell], ...]:
    remaining = cells.copy()
    result: list[frozenset[Cell]] = []
    while remaining:
        seed = min(remaining)
        remaining.remove(seed)
        found = {seed}
        pending = [seed]
        while pending:
            current = pending.pop()
            for axis in range(3):
                for sign in (-1, 1):
                    neighbour = tuple(current[a] + (sign if a == axis else 0) for a in range(3))
                    if neighbour in remaining:
                        remaining.remove(neighbour)
                        found.add(neighbour)
                        pending.append(neighbour)
        if all(
            max(cell[axis] for cell in found) - min(cell[axis] for cell in found) + 1
            >= _MIN_COMPONENT_SPAN
            for axis in range(3)
        ):
            result.append(frozenset(found))
    return tuple(sorted(result, key=lambda cells: min(cells)))


def sample_void_grid(body: Part) -> VoidGrid | None:
    """Sample material and bounded air in one body's caller-supplied XYZ frame."""

    bounds = body.bounding_box()
    origin = (float(bounds.min.X), float(bounds.min.Y), float(bounds.min.Z))
    extents = (float(bounds.size.X), float(bounds.size.Y), float(bounds.size.Z))
    longest = max(extents)
    if not math.isfinite(longest) or longest <= COORD_FLOOR:
        return None
    pitch = longest / _CELLS_ON_LONGEST_AXIS
    dimensions = (
        math.ceil(extents[0] / pitch),
        math.ceil(extents[1] / pitch),
        math.ceil(extents[2] / pitch),
    )
    nx, ny, nz = dimensions
    size = nx * ny * nz
    valid = bytearray(size)
    material_votes = bytearray(size)
    bounded_votes = bytearray(size)
    intersector = IntCurvesFace_ShapeIntersector()
    intersector.Load(body.wrapped, COORD_FLOOR)
    coincidence = length_tol(pitch, rel=1e-6)

    for axis in range(3):
        other = tuple(a for a in range(3) if a != axis)
        direction = tuple(int(a == axis) for a in range(3))
        for first in range(dimensions[other[0]]):
            for second in range(dimensions[other[1]]):
                start = [origin[a] - pitch if a == axis else 0.0 for a in range(3)]
                start[other[0]] = origin[other[0]] + (first + 0.5) * pitch
                start[other[1]] = origin[other[1]] + (second + 0.5) * pitch
                try:
                    intersector.Perform(
                        gp_Lin(gp_Pnt(*start), gp_Dir(*direction)),
                        0.0,
                        (dimensions[axis] + 2) * pitch,
                    )
                    hits = sorted(
                        intersector.WParameter(hit)
                        for hit in range(1, intersector.NbPnt() + 1)
                        if intersector.WParameter(hit) > COORD_FLOOR
                    )
                except (Standard_Failure, Standard_NullObject, RuntimeError, ValueError):
                    continue
                crossings: list[float] = []
                for hit in hits:
                    if not crossings or hit - crossings[-1] > coincidence:
                        crossings.append(hit)
                # A generic line through a valid solid has paired crossings.
                # Odd intersections are usually a grazing edge or a kernel refusal.
                if len(crossings) % 2:
                    continue
                for position in range(dimensions[axis]):
                    distance = (position + 1.5) * pitch
                    if any(abs(distance - hit) <= coincidence for hit in crossings):
                        continue
                    coordinates = [0, 0, 0]
                    coordinates[axis] = position
                    coordinates[other[0]] = first
                    coordinates[other[1]] = second
                    x, y, z = coordinates
                    index = (x * ny + y) * nz + z
                    valid[index] += 1
                    material_votes[index] += bisect_right(crossings, distance) % 2
                    bounded_votes[index] += int(
                        bool(crossings) and crossings[0] < distance < crossings[-1]
                    )

    material: set[Cell] = set()
    air: set[Cell] = set()
    bounded_air: set[Cell] = set()
    air_samples = 0
    for x in range(nx):
        for y in range(ny):
            for z in range(nz):
                index = (x * ny + y) * nz + z
                if valid[index] != 3:
                    continue
                if material_votes[index] == 3:
                    material.add((x, y, z))
                elif material_votes[index] == 0:
                    air_samples += 1
                    air.add((x, y, z))
                    if bounded_votes[index] == 3:
                        bounded_air.add((x, y, z))
    return VoidGrid(
        origin,
        pitch,
        dimensions,
        frozenset(material),
        frozenset(air),
        _components(bounded_air),
        air_samples,
    )
