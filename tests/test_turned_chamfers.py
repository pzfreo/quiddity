# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Turned chamfers are conical rather than planar faces."""

from pathlib import Path

import pytest
from attribution_audit import attributed_run, unattributed_run
from build123d import Axis, Box, Cone, Cylinder, GeomType, Pos, Sphere, Torus, fillet, import_step

from quiddity import (
    Chamfer,
    Fillet,
    build_recognition_result,
    recognise_chamfers,
    recognise_fillets,
)
from quiddity._candidates import FamilyId
from quiddity._geometry import _coaxial_axis_lines

CORPUS = Path(__file__).parent / "corpus" / "gramel"


def _chamfered_stepped_shaft(size=0.8):
    """Two turned diameters, with each free end and shoulder edge chamfered."""

    shaft = Pos(0, 0, 20) * Cylinder(15, 40) + Pos(0, 0, 55) * Cylinder(8, 30)
    circular_edges = [edge for edge in shaft.edges() if edge.geom_type == GeomType.CIRCLE]
    return shaft.chamfer(size, None, circular_edges)


def _filleted_stepped_shaft():
    shaft = Pos(0, 0, 20) * Cylinder(15, 40) + Pos(0, 0, 55) * Cylinder(8, 30)
    circular_edges = [edge for edge in shaft.edges() if edge.geom_type == GeomType.CIRCLE]
    return fillet(circular_edges, 0.8)


def test_direct_reader_recognises_conical_turned_chamfers():
    part = _chamfered_stepped_shaft()
    ledger, found = attributed_run(part, FamilyId.CHAMFERS, recognise_chamfers)

    assert len(found) == 4
    assert {(chamfer.axis, chamfer.leg1, chamfer.leg2, chamfer.angle) for chamfer in found} == {
        ("z", 0.8, 0.8, 45.0)
    }
    assert all(chamfer.turned for chamfer in found)
    for candidate in ledger.candidate_set(FamilyId.CHAMFERS).candidates:
        (node,) = ledger.defining_of(candidate)
        face = ledger.graph.face(node)
        assert face.geom_type == GeomType.CONE
        center = face.center()
        assert (
            tuple(round(value, 3) for value in (center.X, center.Y, center.Z))
            == candidate.record.at
        )


def test_rotational_inventory_keeps_turned_chamfers():
    result = build_recognition_result(_chamfered_stepped_shaft(), rotational=True)

    assert len(result.chamfers) == 4
    assert all(chamfer.axis == "z" and chamfer.turned for chamfer in result.chamfers)


def test_real_turned_inventory_keeps_dimensioned_three_tenths_chamfers():
    part = import_step(str(CORPUS / "GRM-03_thumbwheel_drive_screw.step"))

    ledger, found = attributed_run(part, FamilyId.CHAMFERS, recognise_chamfers)
    assert tuple(found) == build_recognition_result(part, rotational=True).chamfers
    assert all(
        len(ledger.defining_of(candidate)) == 1
        for candidate in ledger.candidate_set(FamilyId.CHAMFERS).candidates
    )

    assert [
        (chamfer.axis, chamfer.leg1, chamfer.leg2, chamfer.angle, chamfer.at) for chamfer in found
    ] == [
        ("x", 0.3, 0.3, 45.0, (0.65, 0.0, 4.85)),
        ("x", 0.3, 0.3, 45.0, (2.35, 0.0, 4.85)),
        ("x", 0.5, 0.5, 45.0, (23.25, 0.0, 1.25)),
    ]


def test_two_tenths_turned_edge_break_is_geometry_by_default_but_obeys_a_caller_floor():
    _ledger, found = attributed_run(
        _chamfered_stepped_shaft(0.2), FamilyId.CHAMFERS, recognise_chamfers
    )
    assert found

    unattributed_run(
        _chamfered_stepped_shaft(0.2),
        FamilyId.CHAMFERS,
        recognise_chamfers,
        kwargs={"tol": 0.3},
    )


def test_rotational_inventory_keeps_toroidal_turned_fillets():
    result = build_recognition_result(_filleted_stepped_shaft(), rotational=True)

    assert len(recognise_fillets(_filleted_stepped_shaft())) == 4
    assert len(result.fillets) == 4
    assert {(fillet.axis, fillet.radius) for fillet in result.fillets} == {("z", 0.8)}
    assert all(fillet.turned for fillet in result.fillets)


def test_prismatic_edge_treatments_and_legacy_constructors_default_to_not_turned():
    box = Box(40, 30, 20)
    vertical_edges = list(box.edges().filter_by(Axis.Z))
    bevelled = box.chamfer(1.0, None, vertical_edges)
    rounded = fillet(vertical_edges, 1.0)

    assert all(not item.turned for item in recognise_chamfers(bevelled))
    assert all(not item.turned for item in recognise_fillets(rounded))
    assert not Chamfer("z", 1.0, 1.0, 45.0, (0.0, 0.0, 0.0)).turned
    assert not Fillet("z", 1.0, (0.0, 0.0, 0.0)).turned


def test_internal_countersink_is_not_a_turned_chamfer():
    part = Box(60, 60, 20) - Cylinder(3, 20) - Pos(0, 0, 7) * Cone(3, 7, 6)

    assert recognise_chamfers(part) == []


def test_internal_toroidal_bore_round_is_not_a_turned_fillet():
    bored = Box(60, 60, 20) - Cylinder(5, 20)
    inner_rim = bored.edges().filter_by(GeomType.CIRCLE)[0]

    assert recognise_fillets(fillet(inner_rim, 1.0)) == []


def test_toroidal_bead_is_not_a_turned_fillet():
    bead = Cylinder(10, 20) + Torus(10, 2)

    assert recognise_fillets(bead) == []


def test_a_full_torus_is_not_a_turned_fillet():
    assert recognise_fillets(Torus(10, 2)) == []


def test_non_principal_turned_surfaces_are_out_of_scope():
    slanted_cone = Cone(10, 8, 2).rotate(Axis.X, 45)
    slanted_bead = (Cylinder(10, 20) + Torus(10, 2)).rotate(Axis.X, 45)

    assert recognise_chamfers(slanted_cone) == []
    assert recognise_fillets(slanted_bead) == []


def test_a_taper_into_a_rounded_nose_is_not_a_turned_chamfer():
    shaft_and_taper = Cylinder(10, 20).fuse(Pos(0, 0, 13) * Cone(10, 8, 6))
    rounded_nose = shaft_and_taper.fuse(Pos(0, 0, 16) * Sphere(8))

    assert recognise_chamfers(rounded_nose) == []


def test_coaxial_line_evidence_requires_three_component_directions():
    with pytest.raises(ValueError, match="directions must be 3-vectors"):
        _coaxial_axis_lines((0, 0, 0), (0, 1), (0, 0, 0), (0, 0, 1), tol=0.01)
