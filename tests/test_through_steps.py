# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Rectangular through-step recognition and its open-profile boundaries."""

from __future__ import annotations

from dataclasses import replace

import pytest
from build123d import (
    Box,
    BuildPart,
    BuildSketch,
    Compound,
    Cylinder,
    Edge,
    Face,
    Plane,
    Polygon,
    Pos,
    Rot,
    Shell,
    Solid,
    Vector,
    Wire,
    export_step,
    extrude,
    import_step,
)

from quiddity import build_recognition_result, feature_census
from quiddity import through_steps as through_step_module
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._dispositions import Outcome
from quiddity.result import _take_inventory
from quiddity.through_steps import (
    ThroughStep,
    _four_principal_runs,
    recognise_through_steps,
)


def _step(scale: float = 1.0):
    stock = Box(40 * scale, 30 * scale, 20 * scale)
    removal = Pos(15 * scale, 10 * scale, 0) * Box(20 * scale, 20 * scale, 30 * scale)
    return stock - removal


def _geometry_only(records):
    return [replace(record, body_key=()) for record in records]


def _split_face_at_z(part, wall, at: float = 0.0) -> Solid:
    low = sorted((tuple(v) for v in wall.vertices() if at > v.Z), key=lambda p: p[:2])
    high = sorted((tuple(v) for v in wall.vertices() if at < v.Z), key=lambda p: p[:2])
    middle = [(point[0], point[1], at) for point in low]

    def oriented(*points):
        face = Face(Wire.make_polygon([Vector(*point) for point in points], close=True))
        if face.normal_at().dot(wall.normal_at()) < 0:
            face = Face(Wire.make_polygon([Vector(*point) for point in points[::-1]], close=True))
        return face

    patches = [
        oriented(low[0], low[1], middle[1], middle[0]),
        oriented(middle[0], middle[1], high[1], high[0]),
    ]
    solid = Solid(Shell([*(f for f in part.faces() if not f.is_same(wall)), *patches]))
    assert solid.is_valid
    return solid


def _split_upper_terminal(part) -> Solid:
    terminal = max(part.faces(), key=lambda face: face.center().Z)

    def upward(*points):
        face = Face(Wire.make_polygon([Vector(*point) for point in points], close=True))
        if face.normal_at().Z < 0:
            face = Face(Wire.make_polygon([Vector(*point) for point in points[::-1]], close=True))
        return face

    z = terminal.center().Z
    patches = (
        upward((-20, -15, z), (5, -15, z), (5, 15, z), (-20, 15, z)),
        upward((5, -15, z), (20, -15, z), (20, 0, z), (5, 0, z)),
    )
    solid = Solid(Shell([*(face for face in part.faces() if not face.is_same(terminal)), *patches]))
    assert solid.is_valid
    return solid


def test_rectangular_through_step_has_one_canonical_open_section_and_claim():
    part = _step()
    ledger = ClaimLedger(FaceGraph(part))

    assert _geometry_only(recognise_through_steps(part, ledger=ledger)) == [
        ThroughStep(
            axis="z",
            length=20.0,
            at=(12.5, 7.5, 0.0),
            section=((5.0, 15.0), (5.0, 0.0), (20.0, 0.0)),
        )
    ]
    assert len(ledger.claims) == 1
    assert len(ledger.claims[0].defining) == 2


def test_aggregate_candidate_result_and_census_are_one_accepted_occurrence():
    part = _step()
    product = _take_inventory(part)
    candidate_set = product.physical.candidate_set(FamilyId.THROUGH_STEPS)

    assert len(candidate_set.candidates) == 1
    assert [item.outcome for item in product.reconciliation.for_family(FamilyId.THROUGH_STEPS)] == [
        Outcome.ACCEPTED
    ]
    assert len(product.evidence.defining_of(candidate_set.candidates[0])) == 2
    assert product.result.through_steps == tuple(recognise_through_steps(part))
    assert build_recognition_result(part).through_steps == product.result.through_steps
    assert feature_census(part)["through_step"] == 1


def test_record_supports_run_and_open_section_dimension_projection():
    step = recognise_through_steps(_step())[0]
    start, corner, end = step.section

    assert {
        "leader": step.at,
        "run": (step.axis, step.length),
        "legs": (
            abs(start[0] - corner[0]) + abs(start[1] - corner[1]),
            abs(end[0] - corner[0]) + abs(end[1] - corner[1]),
        ),
    } == {"leader": (12.5, 7.5, 0.0), "run": ("z", 20.0), "legs": (15.0, 15.0)}


def test_rectangular_step_is_rotation_mirror_scale_and_step_roundtrip_stable(tmp_path):
    base = _step()
    path = tmp_path / "through-step.step"
    export_step(base, path)
    assert recognise_through_steps(import_step(path)) == recognise_through_steps(base)

    for part, axis in (
        (base, "z"),
        (Rot(90, 0, 0) * base, "y"),
        (Rot(0, 90, 0) * base, "x"),
        (Rot(0, 0, 180) * base, "z"),
    ):
        assert [step.axis for step in recognise_through_steps(part)] == [axis]

    assert _geometry_only(recognise_through_steps(Rot(90, 0, 0) * base)) == [
        ThroughStep("y", 20.0, (12.5, 0.0, 7.5), ((5.0, 15.0), (5.0, -0.0), (20.0, -0.0)))
    ]
    assert _geometry_only(recognise_through_steps(Rot(0, 90, 0) * base)) == [
        ThroughStep("x", 20.0, (0.0, 7.5, -12.5), ((0.0, -20.0), (0.0, -5.0), (15.0, -5.0)))
    ]
    mirrored = recognise_through_steps(base.mirror(Plane.YZ))
    assert len(mirrored) == 1
    assert (mirrored[0].axis, mirrored[0].length) == ("z", 20.0)

    for scale in (0.001, 1000.0):
        (step,) = recognise_through_steps(_step(scale))
        assert step.length == pytest.approx(20 * scale, abs=max(0.001, scale * 1e-6))


def test_coplanar_face_subdivision_is_representation_only():
    plain = _step()
    defining = next(
        face
        for face in plain.faces()
        if abs(face.center().X - 5) < 1e-8 and abs(face.center().Z) < 1e-8
    )
    split = _split_face_at_z(plain, defining)
    ledger = ClaimLedger(FaceGraph(split))

    assert recognise_through_steps(split, ledger=ledger) == recognise_through_steps(plain)
    assert len(ledger.claims[0].defining) == 3


def test_coplanar_terminal_subdivision_is_representation_only():
    plain = _step()
    split = _split_upper_terminal(plain)

    assert recognise_through_steps(split) == recognise_through_steps(plain)
    assert recognise_through_steps(Rot(180, 0, 0) * split) == recognise_through_steps(
        Rot(180, 0, 0) * plain
    )


def test_channels_pockets_and_slots_are_not_through_steps():
    stock = Box(40, 30, 20)
    channel = stock - Box(20, 10, 30)
    pocket = stock - Pos(0, 0, 5) * Box(20, 10, 10)
    slot = stock - Pos(0, 10, 0) * Box(20, 20, 30)

    for part in (channel, pocket, slot):
        assert recognise_through_steps(part) == []


def test_a_third_cospanning_concave_wall_is_not_one_open_step():
    channel = Box(40, 30, 20) - Box(20, 10, 30)

    assert recognise_through_steps(channel) == []


def test_material_inside_the_inferred_removed_prism_fails_closed():
    rib_into_void = Pos(10, 7.5, 0) * Box(10, 2, 20)
    obstructed = _step() + rib_into_void

    assert obstructed.is_valid
    assert recognise_through_steps(obstructed) == []


def test_an_additive_l_solid_has_the_same_history_free_geometry():
    """A final B-rep cannot distinguish adding an L arm from removing its complement."""

    additive = Box(20, 30, 20) + Pos(15, -10, 0) * Box(10, 10, 20)

    assert len(recognise_through_steps(additive)) == 1


def test_a_step_capped_at_one_run_end_is_blind_not_through():
    stock = Box(40, 30, 20)
    removal = Pos(15, 10, 5) * Box(20, 20, 10)

    assert recognise_through_steps(stock - removal) == []


def test_a_convex_straight_boundary_notch_preserves_the_step_proof():
    part = _step() - Pos(5, 14, 0) * Box(4, 4, 4)
    defining_wall = next(face for face in part.faces() if pytest.approx(5.0) == face.center().X)

    assert len(defining_wall.wires()) == 1
    assert len(defining_wall.edges()) == 8
    assert _geometry_only(recognise_through_steps(part)) == _geometry_only(
        recognise_through_steps(_step())
    )


def test_a_tapered_defining_wall_is_not_a_constant_section_step():
    with BuildPart() as removal:
        with BuildSketch(Plane.XY.offset(-10)):
            Polygon((5, 0), (20, 0), (20, 15), (8, 15))
        extrude(amount=20)
    tapered = Box(40, 30, 20) - removal.part

    assert recognise_through_steps(tapered) == []
    assert recognise_through_steps(Rot(0, 0, 180) * tapered) == []
    assert recognise_through_steps(Rot(90, 0, 0) * tapered) == []


def test_four_curved_edges_are_not_four_straight_principal_runs():
    point = Vector
    wire = Wire(
        [
            Edge.make_bezier(point(-1, -1), point(-0.5, -1.4), point(0.5, -1.4), point(1, -1)),
            Edge.make_bezier(point(1, -1), point(1.4, -0.5), point(1.4, 0.5), point(1, 1)),
            Edge.make_bezier(point(1, 1), point(0.5, 1.4), point(-0.5, 1.4), point(-1, 1)),
            Edge.make_bezier(point(-1, 1), point(-1.4, 0.5), point(-1.4, -0.5), point(-1, -1)),
        ]
    )

    assert wire.is_closed
    assert not _four_principal_runs(wire, 2)


def test_a_curved_boundary_interruption_preserves_the_step_proof():
    scallop = Pos(6, 15, 0) * Rot(0, 90, 0) * Cylinder(10, 4)
    part = _step() - scallop

    assert _geometry_only(recognise_through_steps(part)) == _geometry_only(
        recognise_through_steps(_step())
    )
    assert _geometry_only(recognise_through_steps(Rot(0, 0, 180) * part)) == _geometry_only(
        recognise_through_steps(Rot(0, 0, 180) * _step())
    )


def test_an_inner_wire_interruption_is_rotation_scale_and_step_stable(tmp_path):
    def pierced(scale: float):
        drill = Pos(0, 7 * scale, 0) * Rot(0, 90, 0) * Cylinder(2 * scale, 15 * scale)
        return _step(scale) - drill

    for scale in (0.05, 1.0, 100.0):
        part = pierced(scale)
        assert _geometry_only(recognise_through_steps(part)) == _geometry_only(
            recognise_through_steps(_step(scale))
        )
        assert _geometry_only(recognise_through_steps(Rot(90, 0, 0) * part)) == _geometry_only(
            recognise_through_steps(Rot(90, 0, 0) * _step(scale))
        )

    path = tmp_path / "pierced.step"
    export_step(pierced(1.0), path)
    assert _geometry_only(recognise_through_steps(import_step(path))) == _geometry_only(
        recognise_through_steps(_step())
    )


def test_an_interrupted_wall_keeps_exact_graph_owned_evidence_and_aggregate_parity():
    drill = Pos(0, 7, 0) * Rot(0, 90, 0) * Cylinder(2, 15)
    part = _step() - drill
    graph = FaceGraph(part)
    ledger = ClaimLedger(graph)

    direct = recognise_through_steps(part, ledger=ledger)

    assert direct == recognise_through_steps(part)
    assert len(ledger.claims) == 1
    defining = ledger.claims[0].defining
    assert len(defining) == 2
    assert all(graph.require_node(graph.face(node)) is node for node in defining)
    assert any(len(graph.face(node).wires()) > 1 for node in defining)

    product = _take_inventory(part)
    candidates = product.physical.candidate_set(FamilyId.THROUGH_STEPS).candidates
    assert len(candidates) == 1
    aggregate_defining = product.evidence.defining_of(candidates[0])
    assert product.result.through_steps == tuple(direct)
    assert len(aggregate_defining) == 2
    assert all(
        product.context.graph.require_node(product.context.graph.face(node)) is node
        for node in aggregate_defining
    )
    assert any(len(product.context.graph.face(node).wires()) > 1 for node in aggregate_defining)


def test_an_interruption_crossing_the_defining_seam_fails_closed():
    interrupted = _step() - Pos(5, 0, 0) * Cylinder(2, 20)

    assert interrupted.is_valid
    assert recognise_through_steps(interrupted) == []


def test_compound_members_are_recognised_independently():
    first = _step()
    second = Pos(100, 0, 0) * _step()

    assert len(recognise_through_steps(Compound(children=[first, second]))) == 2

    forward = recognise_through_steps(Compound(children=[first, second]))
    reverse = recognise_through_steps(Compound(children=[second, first]))
    assert forward == reverse


def test_open_body_and_cross_solid_final_shape_composite_are_refused():
    assert recognise_through_steps(Shell(list(_step().faces()))) == []

    vertical_arm = Box(20, 30, 20)
    horizontal_arm = Pos(15, -10, 0) * Box(10, 10, 20)
    cross_solid = Compound(children=[vertical_arm, horizontal_arm])
    assert recognise_through_steps(cross_solid) == []


def test_foreign_evidence_fails_before_any_family_candidate_is_issued(monkeypatch):
    part = _step()
    ledger = ClaimLedger(FaceGraph(part))
    foreign = FaceGraph(Pos(100, 0, 0) * _step())
    record = recognise_through_steps(part)[0]
    monkeypatch.setattr(
        through_step_module,
        "_recognise_one",
        lambda *_args: [(record, tuple(foreign.nodes[:2]))],
    )

    with pytest.raises(ValueError, match="graph|issued|belong"):
        recognise_through_steps(part, ledger=ledger)
    assert ledger.candidate_set(FamilyId.THROUGH_STEPS).candidates == ()


def test_multiple_steps_on_one_solid_keep_feature_local_locations():
    stock = Box(80, 50, 20)
    part = stock - Pos(30, 20, 0) * Box(20, 20, 30) - Pos(-30, -20, 0) * Box(20, 20, 30)

    steps = recognise_through_steps(part)

    assert len(steps) == 2
    assert {step.at for step in steps} == {(-30.0, -17.5, 0.0), (30.0, 17.5, 0.0)}
