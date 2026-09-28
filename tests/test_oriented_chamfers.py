# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Proved external chamfers on oblique edges."""

from dataclasses import replace

from build123d import Box, Pos, Rot, Shell, Solid, chamfer, export_step, import_step

from quiddity import build_recognition_result, feature_census
from quiddity._adjacency import FaceGraph
from quiddity._claims import ClaimLedger
from quiddity.oriented_chamfers import (
    OrientedChamfer,
    _discover_oriented_chamfers,
    recognise_oriented_chamfers,
)
from tests.golden.gusset_ribs.fixture import build_fixture as gusset_fixture


def _bevel(*, rotated: bool = True):
    box = Rot(0, 0, 30) * Box(40, 30, 20) if rotated else Box(40, 30, 20)
    edge = next(edge for edge in box.edges() if abs(edge.center().Z - 10) < 1e-6)
    return chamfer(edge, length=2)


def test_external_oblique_bevel_has_positive_oriented_legs_and_one_defining_face():
    part = _bevel()
    ledger = ClaimLedger(FaceGraph(part))
    records = _discover_oriented_chamfers(part, graph=ledger.graph, sink=ledger.sink)

    assert len(records) == 1
    assert replace(records[0], body_key=()) == OrientedChamfer(
        run=(0.5, -0.866025, 0.0),
        length=30.0,
        at=(-16.454, -9.5, 9.0),
        corner=(-17.321, -10.0, 10.0),
        leg1=2.0,
        leg2=2.0,
        leg1_direction=(0.0, 0.0, -1.0),
        leg2_direction=(0.866025, 0.5, 0.0),
        support_spans=((-15.0, 15.0), (-15.0, 15.0)),
        angle=45.0,
    )
    assert len(ledger.claims) == 1
    assert len(ledger.claims[0].defining) == 1
    assert records[0].to_dict()["corner"] == records[0].corner


def test_aggregate_census_and_principal_edge_exclusion():
    part = _bevel()
    assert build_recognition_result(part).oriented_chamfers == tuple(
        recognise_oriented_chamfers(part)
    )
    assert feature_census(part)["oriented_chamfer"] == 1
    assert recognise_oriented_chamfers(_bevel(rotated=False)) == []


def test_local_rotated_boss_bevel_survives_a_principal_stock_frame():
    stock = Box(80, 80, 10)
    boss = Pos(0, 0, 10) * Rot(0, 0, 30) * Box(20, 20, 10)
    edge = next(edge for edge in boss.edges() if abs(edge.center().Z - 15) < 1e-6)
    part = stock + chamfer(edge, length=1)
    assert part.is_valid
    assert len(recognise_oriented_chamfers(part)) == 1


def test_step_face_order_scale_and_rotation_preserve_one_occurrence(tmp_path):
    part = _bevel()
    expected = [replace(record, body_key=()) for record in recognise_oriented_chamfers(part)]
    path = tmp_path / "oriented-chamfer.step"
    export_step(part, path)
    assert [
        replace(record, body_key=()) for record in recognise_oriented_chamfers(import_step(path))
    ] == expected
    reordered = Solid(Shell(list(reversed(part.faces()))))
    assert reordered.is_valid
    assert [
        replace(record, body_key=()) for record in recognise_oriented_chamfers(reordered)
    ] == expected
    for rotation in (Rot(90, 0, 0), Rot(0, 90, 0), Rot(0, 0, 180)):
        assert len(recognise_oriented_chamfers(rotation * part)) == 1
    for scale in (0.05, 100.0):
        assert len(recognise_oriented_chamfers(part.scale(scale))) == 1


def test_material_side_and_size_gates_reject_large_or_unproved_bevels():
    assert recognise_oriented_chamfers(_bevel(), max_leg_frac=0.01) == []
    # A rotated uncut box has the same oblique support edges but no replacement face.
    assert recognise_oriented_chamfers(Rot(0, 0, 30) * Box(40, 30, 20)) == []


def test_rotated_blind_step_and_gussets_are_not_oriented_chamfers():
    wedge = Rot(0, 0, 30) * (
        Box(60, 40, 12) - Pos(-20, 20, 6) * Rot(45, 0, 0) * Box(30, 5.657, 5.657)
    )
    assert recognise_oriented_chamfers(wedge) == []
    assert recognise_oriented_chamfers(Rot(0, 0, 30) * gusset_fixture()) == []
