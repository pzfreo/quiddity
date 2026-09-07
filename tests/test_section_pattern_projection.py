"""Public pattern positions come from the exact accepted SectionRecess members (#555)."""

import math
from dataclasses import replace

import pytest
from build123d import Box, Compound, Cylinder, Pos, Rot, export_step

from quiddity import (
    SectionRecessArray,
    SectionRecessGrid,
    build_raw_recognition_result,
    build_section_recess_document,
    import_step_geometry,
)
from quiddity.result import _project_section_pattern
from tools._legacy_recognition import build_raw_recognition_result as legacy_result


def _curved_pockets(*, grid=False, scale=1):
    if grid:
        part = Pos(0, 0, -20 * scale) * Box(130 * scale, 110 * scale, 4 * scale)
        for y in (-30, 30):
            part += Pos(0, y * scale, 0) * Rot(0, 90, 0) * Cylinder(20 * scale, 120 * scale)
    else:
        part = Rot(0, 90, 0) * Cylinder(20 * scale, 100 * scale)
    for y in (-21, 39) if grid else (9,):
        for x in (-30, 0, 30):
            part -= Pos(x * scale, y * scale, 14 * scale) * Box(6 * scale, 6 * scale, 12 * scale)
    return part


def _midpoint(record):
    g = record.geometry
    return tuple(g.frame.origin[i] + sum(g.run_interval) / 2 * g.frame.run[i] for i in range(3))


@pytest.mark.parametrize("grid", [False, True])
@pytest.mark.parametrize("scale", [0.1, 1, 10])
@pytest.mark.parametrize("rotation", [Rot(), Rot(90, 0, 0), Rot(0, 90, 0), Rot(180, 0, 0)])
def test_one_pattern_reconstructs_its_exact_members(grid, scale, rotation):
    placement = Pos(123.0004, -57.0004, 91.0004) * rotation
    result = build_raw_recognition_result(placement * _curved_pockets(grid=grid, scale=scale))
    assert len(result.section_recesses) == (6 if grid else 3)
    assert not result.section_recess_refusals
    (pattern,) = result.section_recess_patterns
    members = [result.section_recesses[index] for index in pattern.members]
    actual = [_midpoint(record) for record in members]
    if grid:
        assert isinstance(pattern, SectionRecessGrid)
        expected = [
            tuple(
                pattern.center[i]
                + (row - (pattern.rows - 1) / 2) * pattern.row_pitch * pattern.row_direction[i]
                + (col - (pattern.cols - 1) / 2) * pattern.col_pitch * pattern.col_direction[i]
                for i in range(3)
            )
            for row in range(pattern.rows)
            for col in range(pattern.cols)
        ]
        assert pattern.center == pytest.approx(
            tuple(math.fsum(point[i] for point in actual) / len(actual) for i in range(3))
        )
    else:
        assert isinstance(pattern, SectionRecessArray)
        expected = [
            tuple(actual[0][i] + at * pattern.pitch * pattern.direction[i] for i in range(3))
            for at in range(len(actual))
        ]
    for point, target in zip(actual, expected, strict=True):
        assert math.dist(point, target) <= 0.002


def test_reported_grid_uses_centroid_reference_end_midpoints():
    document = build_section_recess_document(_curved_pockets(grid=True))
    (pattern,) = document.patterns
    assert pattern.center == pytest.approx((0, 9, 12.9305))
    assert pattern.members == (0, 1, 2, 3, 4, 5)
    assert len(document.to_dict()["patterns"]) == 1


def test_patterns_on_distinct_bodies_are_not_deduplicated_together():
    first = _curved_pockets()
    result = build_raw_recognition_result(Compound([first, Pos(200, 0, 0) * first]))
    assert len(result.section_recess_patterns) == 2
    one, two = result.section_recess_patterns
    assert set(one.members).isdisjoint(two.members)


def test_pattern_survives_step_round_trip(tmp_path):
    path = tmp_path / "curved-grid.step"
    assert export_step(_curved_pockets(grid=True), path)
    document = build_section_recess_document(import_step_geometry(path))
    (pattern,) = document.patterns
    assert pattern.center == pytest.approx((0, 9, 12.9305), abs=0.002)
    assert set(pattern.members) == set(range(6))


@pytest.mark.parametrize("grid", [False, True])
def test_legacy_lattice_cannot_override_inconsistent_accepted_member_positions(grid):
    result = legacy_result(_curved_pockets(grid=grid))
    pattern = result.pocket_patterns[0]
    records = result.section_recesses
    assert _project_section_pattern(pattern, records) is not None
    changed = replace(
        records[0],
        geometry=replace(
            records[0].geometry,
            frame=replace(
                records[0].geometry.frame,
                origin=tuple(
                    records[0].geometry.frame.origin[i] + records[0].geometry.frame.u[i]
                    for i in range(3)
                ),
            ),
        ),
    )
    assert _project_section_pattern(pattern, (changed, *records[1:])) is None
    assert _project_section_pattern(pattern, (replace(records[0], body=99), *records[1:])) is None
