# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Issue #586: cylindrical plate corners do not establish a turned profile."""

import pytest
from attribution_audit import unattributed_run
from build123d import Axis, Box, Cylinder, Pos, Rotation

from quiddity import build_recognition_result, recognise_plates, recognise_turned_steps
from quiddity._candidates import FamilyId


def _rounded_plate(radius):
    plate = Box(70, 70, 3)
    return plate.fillet(radius, plate.edges().filter_by(Axis.Z))


def _rounded_slabs(separated):
    lower = Pos(0, 0, 1.5) * _rounded_plate(25)
    upper = Pos(0, 0, 35.5 if separated else 4.5) * _rounded_plate(3.5)
    part = lower + upper
    if separated:
        part += Pos(0, 0, 18.5) * Box(3, 60, 37)
    return part


@pytest.mark.parametrize("separated", [False, True])
@pytest.mark.parametrize("axis", ["x", "y", "z"])
@pytest.mark.parametrize("scale", [0.05, 1, 5])
def test_rounded_slabs_do_not_publish_steps(separated, axis, scale):
    # Touching slabs have 100% axial coverage: a coverage gate cannot fix this.
    part = _rounded_slabs(separated).scale(scale)
    rotation = {"x": Rotation(0, 90, 0), "y": Rotation(90, 0, 0), "z": Rotation(0, 0, 0)}
    part = Pos(91, -37, 48) * rotation[axis] * part
    assert len(part.solids()) == 1
    unattributed_run(part, FamilyId.TURNED_STEPS, recognise_turned_steps)


def test_false_steps_do_not_suppress_sheet_plates():
    part = _rounded_slabs(True)
    plates = recognise_plates(part)
    assert len(plates) == 3
    result = build_recognition_result(part)
    assert result.turned_steps == ()
    assert result.plates == tuple(plates)
    assert len(result.fillets) == 8


def test_coaxial_quarter_cylinders_do_not_establish_turned_diameters():
    shaft = Pos(0, 0, 1.5) * Cylinder(25, 3) + Pos(0, 0, 4.5) * Cylinder(3.5, 3)
    quarter = shaft & (Pos(25, 25, 3) * Box(50, 50, 6))
    assert len(quarter.solids()) == 1
    unattributed_run(quarter, FamilyId.TURNED_STEPS, recognise_turned_steps)


def test_parallel_offset_cylinders_do_not_form_one_profile():
    # Both bands have full angular support, but their axis lines differ.
    part = Pos(0, 0, 1.5) * Cylinder(25, 3) + Pos(10, 0, 4.5) * Cylinder(3.5, 3)
    assert len(part.solids()) == 1
    unattributed_run(part, FamilyId.TURNED_STEPS, recognise_turned_steps)


def test_genuine_thin_steps_are_preserved():
    # Equal 3 mm lengths are valid; material thickness is not a refusal rule.
    part = Pos(0, 0, 1.5) * Cylinder(25, 3) + Pos(0, 0, 4.5) * Cylinder(3.5, 3)
    steps = recognise_turned_steps(part)
    assert [(step.lo, step.hi, step.diameter) for step in steps] == [
        (0, 3, 50),
        (3, 6, 7),
    ]
