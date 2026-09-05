# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Provider-owned lattice conventions formerly checked through Draftwright private imports."""

import math

import pytest
from build123d import Box, Pos, Rot

from quiddity import (
    HoleRecord,
    RectGrid,
    SectionRecessGrid,
    Slot,
    SlotGrid,
    build_section_recess_document,
    recognise_hole_patterns,
    recognise_slot_patterns,
)
from quiddity._pattern_geometry import _plane_uv

BASES = {
    "x": ((0, 1, 0), (0, 0, 1)),
    "y": ((0, 0, 1), (1, 0, 0)),
    "z": ((1, 0, 0), (0, 1, 0)),
}
LATTICES = [
    (2, 5, 10, 45),
    (5, 2, 45, 10),
    (3, 4, 17, 31),
    (4, 3, 31, 17),
    (2, 3, 45, 40),
    (3, 3, 25, 25),
]


def lattice(axis, rows, cols, row_pitch, col_pitch, angle, center=(13, -7, 5)):
    # Independent reconstruction of the documented convention, not the detector's basis code.
    u, v = BASES[axis]
    c, s = math.cos(math.radians(angle)), math.sin(math.radians(angle))
    return [
        tuple(
            center[i]
            + (col - (cols - 1) / 2) * col_pitch * (c * u[i] + s * v[i])
            + (row - (rows - 1) / 2) * row_pitch * (-s * u[i] + c * v[i])
            for i in range(3)
        )
        for row in range(rows)
        for col in range(cols)
    ]


def assert_same_points(actual, expected, *, tolerance=0.002):
    assert len(actual) == len(expected)
    remaining = list(expected)
    for point in actual:
        matches = [
            i
            for i, other in enumerate(remaining)
            if point == pytest.approx(other, abs=tolerance, rel=0)
        ]
        assert len(matches) == 1
        remaining.pop(matches[0])
    assert remaining == []


@pytest.mark.parametrize("axis", "xyz")
@pytest.mark.parametrize("angle", [0, 25, 37, 73])
@pytest.mark.parametrize("dimensions", LATTICES)
@pytest.mark.parametrize("family", ["holes", "slots"])
def test_public_grid_preserves_lattice_count_pitch_and_input_order(axis, angle, dimensions, family):
    points = lattice(axis, *dimensions, angle)
    if family == "holes":
        normal = tuple(int(letter == axis) for letter in "xyz")
        members = [HoleRecord(normal, point, 2, 5, "through") for point in points]
        recognise, grid_type = recognise_hole_patterns, RectGrid
    else:
        width_axis, long_axis = [letter for letter in "xyz" if letter != axis]
        wi, li, di = map("xyz".index, (width_axis, long_axis, axis))
        members = [
            Slot(
                width_axis,
                long_axis,
                2,
                4,
                point[wi],
                point[li] - 2,
                point[li] + 2,
                point[di] - 3,
                point[di] + 3,
            )
            for point in points
        ]
        recognise, grid_type = recognise_slot_patterns, SlotGrid
    signatures = []
    for ordered in (members, list(reversed(members)), members[1:] + members[:1]):
        (grid,) = recognise(ordered)
        assert isinstance(grid, grid_type)
        rows, cols, rp, cp = dimensions
        assert {(grid.rows, grid.row_pitch), (grid.cols, grid.col_pitch)} == {
            (rows, rp),
            (cols, cp),
        }
        signature = (grid.rows, grid.cols, grid.row_pitch, grid.col_pitch, grid.angle)
        signatures.append(signature)
        assert_same_points(lattice(axis, *signature, center=grid.center), points)
    assert signatures[0] == signatures[1] == signatures[2]


@pytest.mark.parametrize(
    "axis",
    [
        (0.04, 0, 1),
        (0, 1, 0.001),
        (1, -0.005, 0.002),
        (0, 0.6, 0.8),
        (0.6, -0.48, 0.64),
        (0, 0, -1),
        (1, 1 - 1e-9, 0),
        (1 - 1e-9, 1, 0),
    ],
)
def test_near_axis_and_tied_basis_is_orthonormal_and_sign_independent(axis):
    u, v = _plane_uv(axis)
    unit = tuple(component / math.hypot(*axis) for component in axis)
    for basis in (u, v):
        assert math.hypot(*basis) == pytest.approx(1, abs=1e-12)
        assert sum(a * b for a, b in zip(basis, unit, strict=True)) == pytest.approx(0, abs=1e-12)
    assert sum(a * b for a, b in zip(u, v, strict=True)) == pytest.approx(0, abs=1e-12)
    assert _plane_uv(tuple(-component for component in axis)) == (u, v)


@pytest.mark.parametrize("axis", "xyz")
def test_principal_basis_matches_the_public_angle_convention(axis):
    assert _plane_uv(tuple(int(letter == axis) for letter in "xyz")) == BASES[axis]


@pytest.mark.parametrize("rotation", [Rot(), Rot(90, 0, 0), Rot(0, 90, 0)])
@pytest.mark.parametrize("pitches", [(15, 30), (30, 15)])
@pytest.mark.parametrize("angle", [25, 73])
def test_unified_pocket_grid_directions_reconstruct_rotated_lattice(rotation, pitches, angle):
    points = lattice("z", 2, 3, *pitches, angle, center=(0, 0, 7))
    part = Box(150, 150, 20)
    for point in points:
        part -= Pos(*point) * Box(4, 8, 6)
    document = build_section_recess_document(rotation * part)
    assert document.refusals == ()
    assert len(document.occurrences) == 6
    (grid,) = document.patterns
    assert isinstance(grid, SectionRecessGrid)
    assert set(grid.members) == set(range(6))
    # This physical route derives patterns from two-decimal detector centres. Two endpoint
    # roundings plus pitch rounding bound pitch error by 0.02, unlike exact authored records.
    assert sorted((grid.rows, grid.cols)) == [2, 3]
    by_count = {grid.rows: grid.row_pitch, grid.cols: grid.col_pitch}
    assert by_count[2] == pytest.approx(pitches[0], abs=0.02)
    assert by_count[3] == pytest.approx(pitches[1], abs=0.02)
    reconstructed = [
        tuple(
            grid.center[i]
            + (row - (grid.rows - 1) / 2) * grid.row_pitch * grid.row_direction[i]
            + (col - (grid.cols - 1) / 2) * grid.col_pitch * grid.col_direction[i]
            for i in range(3)
        )
        for row in range(grid.rows)
        for col in range(grid.cols)
    ]
    assert_same_points(
        reconstructed,
        [tuple((rotation * Pos(*point)).position) for point in points],
        tolerance=0.03,
    )
