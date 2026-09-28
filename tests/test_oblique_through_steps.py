# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Oblique through-step proofs in the caller's principal frame."""

from dataclasses import replace

from build123d import Align, Box, Face, Rot, Shell, Solid, Vector, Wire, export_step, import_step

from quiddity import build_recognition_result, feature_census
from quiddity._adjacency import FaceGraph
from quiddity._claims import ClaimLedger
from quiddity.oblique_through_steps import (
    ObliqueThroughStep,
    _discover_oblique_through_steps,
    recognise_oblique_through_steps,
)


def _cut(profile: list[tuple[float, float]]) -> Solid:
    wire = Wire.make_polygon([Vector(0, y, z) for y, z in profile], close=True)
    return Solid.extrude(Face(wire), Vector(20, 0, 0))


def _step() -> Solid:
    stock = Box(40, 40, 20, align=(Align.MIN, Align.MIN, Align.MIN))
    return stock - _cut([(0, 0), (40, 0), (20, 20), (0, 20)])


def test_oblique_step_publishes_the_measured_wall_outline_and_claim():
    part = _step()
    ledger = ClaimLedger(FaceGraph(part))
    records = _discover_oblique_through_steps(part, graph=ledger.graph, sink=ledger.sink)

    assert len(records) == 1
    record = replace(records[0], body_key=())
    assert record == ObliqueThroughStep(
        run=(0.0, 0.707107, -0.707107),
        length=28.284,
        at=(20.0, 30.0, 10.0),
        depth_direction=(-1.0, 0.0, 0.0),
        depth=20.0,
        across_direction=(0.0, -0.707107, -0.707107),
        wall_outline=((-14.142, 14.142), (0.0, 0.0), (28.284, 0.0), (0.0, 28.284)),
    )
    assert len(ledger.claims) == 1
    assert len(ledger.claims[0].defining) == 2
    assert record.to_dict()["wall_outline"] == record.wall_outline


def test_oblique_step_is_in_aggregate_and_census():
    part = _step()
    result = build_recognition_result(part)
    assert result.oblique_through_steps == tuple(recognise_oblique_through_steps(part))
    assert feature_census(part)["oblique_through_step"] == 1


def test_triangular_and_locally_ended_wedges_remain_unclaimed():
    stock = Box(40, 40, 20, align=(Align.MIN, Align.MIN, Align.MIN))
    triangular = stock - _cut([(0, 0), (40, 0), (0, 20)])
    partial = stock - _cut([(0, 5), (30, 5), (20, 15), (0, 15)])
    assert recognise_oblique_through_steps(triangular) == []
    assert recognise_oblique_through_steps(partial) == []


def test_step_survives_step_round_trip_face_order_scale_and_principal_rotations(tmp_path):
    part = _step()
    expected = [replace(record, body_key=()) for record in recognise_oblique_through_steps(part)]
    path = tmp_path / "oblique-through-step.step"
    export_step(part, path)
    assert [
        replace(record, body_key=())
        for record in recognise_oblique_through_steps(import_step(path))
    ] == expected
    reordered = Solid(Shell(list(reversed(part.faces()))))
    assert reordered.is_valid
    assert [
        replace(record, body_key=()) for record in recognise_oblique_through_steps(reordered)
    ] == expected
    for rotation in (Rot(90, 0, 0), Rot(0, 90, 0), Rot(0, 0, 180)):
        assert len(recognise_oblique_through_steps(rotation * part)) == 1
    for scale in (0.05, 100.0):
        assert len(recognise_oblique_through_steps(part.scale(scale))) == 1
