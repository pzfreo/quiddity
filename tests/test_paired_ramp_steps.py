# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

from contextlib import suppress
from pathlib import Path

from build123d import (
    Align,
    Axis,
    Box,
    Compound,
    Cylinder,
    Edge,
    Plane,
    Polygon,
    Pos,
    Rot,
    Shell,
    Solid,
    Vector,
    chamfer,
    export_step,
    extrude,
    import_step,
)
from OCP.BRepFeat import BRepFeat_SplitShape

from quiddity import (
    PairedRampStep,
    build_recognition_result,
    feature_census,
    recognise_paired_ramp_steps,
)
from quiddity import paired_ramp_steps as paired_ramp_module
from quiddity._adjacency import FaceGraph
from quiddity._bevel import BevelReject, classify_bevel
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._dispositions import Outcome
from quiddity.result import _take_inventory
from tests.golden.triangular_and_hex_pockets.fixture import build_fixture as pocket_fixture


def _side_cut(
    *,
    scale: float = 1.0,
    asymmetric: bool = False,
    blind: bool = False,
    cycle: int = 0,
    half_height: float = 8.0,
):
    stock = Box(40 * scale, 40 * scale, 30 * scale)
    upper = 11 if asymmetric else half_height
    points = [(0, -half_height * scale), (0, upper * scale), (-10 * scale, 0)]
    points = points[cycle:] + points[:cycle]
    profile = Polygon(*points)
    opening_y = 15 * scale if blind else 20 * scale
    cutter = Pos(20 * scale, opening_y, 0) * extrude(Plane.XZ * profile, 25 * scale)
    return stock - cutter


def _proved_pair_from(part):
    graph = FaceGraph(part)
    bevels = {}
    for node in graph.nodes:
        with suppress(BevelReject):
            bevels[node] = classify_bevel(graph.face(node))
    for left in graph.nodes:
        for right in graph.neighbours(left):
            if (
                right.index > left.index
                and left in bevels
                and right in bevels
                and paired_ramp_module._candidate(graph, left, right, bevels[left], bevels[right])
                is not None
            ):
                return graph, left, right, bevels[left], bevels[right]
    raise AssertionError("authored side cut did not supply its proved ramp pair")


def _proved_pair():
    return _proved_pair_from(_side_cut())


def _two_side_cuts():
    def cutter(center_x):
        return Pos(center_x, 20, 0) * extrude(Plane.XZ * Polygon((0, -6), (0, 6), (-8, 0)), 25)

    return Box(45, 40, 30) - cutter(10) - cutter(30)


def _ramp_boundary_notch(*, upper: bool = False, y: float = 5.0, scale: float = 1.0):
    """Cut a straight notch into the non-terminal boundary of one original ramp face."""

    return Pos(19 * scale, y * scale, (9 if upper else -9) * scale) * Box(
        2 * scale, 2 * scale, 4 * scale
    )


def _ramp_inner_wire(*, upper: bool = False, y: float = 5.0, scale: float = 1.0):
    normal = Vector(0.6246950475544243, 0, -0.7808688094430304 if upper else 0.7808688094430304)
    return Plane(
        origin=(15 * scale, y * scale, (4 if upper else -4) * scale), z_dir=normal
    ) * Cylinder(
        1 * scale,
        6 * scale,
        align=(Align.CENTER, Align.CENTER, Align.CENTER),
    )


def test_a_mirror_ramp_pair_open_to_the_stock_side_is_one_physical_cut() -> None:
    assert recognise_paired_ramp_steps(_side_cut()) == [
        PairedRampStep(axis="y", angle=51.34, length=25.0, at=(10.0, 7.5, 0.0))
    ]


def test_a_shallow_mirror_pair_is_a_step_even_when_neither_face_is_a_chamfer() -> None:
    part = _side_cut(half_height=0.5)
    graph = FaceGraph(part)
    shallow = [
        node
        for node in graph.nodes
        if (normal := graph.normal(node)) is not None
        and sum(abs(component) <= paired_ramp_module.SMOOTH_ARC_GAP for component in normal) == 1
        and max(abs(component) for component in normal) > 0.99
    ]

    assert len(shallow) == 2
    assert all(paired_ramp_module._read_ramp(graph, node) is not None for node in shallow)
    for node in shallow:
        with suppress(BevelReject):
            classify_bevel(graph.face(node))
            raise AssertionError("the shared Chamfer reader unexpectedly accepted a shallow ramp")
    assert recognise_paired_ramp_steps(part) == [
        PairedRampStep(axis="y", angle=87.14, length=25.0, at=(10.0, 7.5, 0.0))
    ]


def test_principal_planes_do_not_become_zero_angle_ramps() -> None:
    graph = FaceGraph(Box(10, 20, 30))

    assert all(paired_ramp_module._read_ramp(graph, node) is None for node in graph.nodes)
    assert recognise_paired_ramp_steps(Box(10, 20, 30)) == []


def test_ramp_reader_refuses_non_planar_or_normal_less_faces(monkeypatch) -> None:
    graph, left, _right, _left_read, _right_read = _proved_pair()

    with monkeypatch.context() as patch:
        patch.setattr(graph, "is_planar", lambda _node: False)
        assert paired_ramp_module._read_ramp(graph, left) is None
    with monkeypatch.context() as patch:
        patch.setattr(graph, "normal", lambda _node: None)
        assert paired_ramp_module._read_ramp(graph, left) is None


def test_ramp_run_direction_uses_the_existing_direction_tolerance(monkeypatch) -> None:
    part = _side_cut(half_height=0.5)
    graph = FaceGraph(part)
    node = next(node for node in graph.nodes if paired_ramp_module._read_ramp(graph, node))
    normal = graph.normal(node)
    assert normal is not None
    run = min(range(3), key=lambda axis: abs(normal[axis]))

    at_boundary = tuple(
        paired_ramp_module.SMOOTH_ARC_GAP if axis == run else component
        for axis, component in enumerate(normal)
    )
    outside_boundary = tuple(
        2 * paired_ramp_module.SMOOTH_ARC_GAP if axis == run else component
        for axis, component in enumerate(normal)
    )
    with monkeypatch.context() as patch:
        patch.setattr(graph, "normal", lambda _node: at_boundary)
        assert paired_ramp_module._read_ramp(graph, node) is not None
    with monkeypatch.context() as patch:
        patch.setattr(graph, "normal", lambda _node: outside_boundary)
        assert paired_ramp_module._read_ramp(graph, node) is None


def test_the_pair_claims_both_original_ramps_and_its_required_terminal() -> None:
    part = _side_cut()
    ledger = ClaimLedger(FaceGraph(part))
    records = recognise_paired_ramp_steps(part, ledger=ledger)
    candidate = ledger.candidate_set_for(FamilyId.PAIRED_RAMP_STEPS, records).candidates[0]

    assert len(ledger.snapshot_index().defining_of(candidate)) == 3
    assert len(ledger.claims) == 1


def test_aggregate_candidate_result_and_census_are_one_accepted_occurrence() -> None:
    part = _side_cut()
    product = _take_inventory(part)
    candidate_set = product.physical.candidate_set(FamilyId.PAIRED_RAMP_STEPS)

    assert len(candidate_set.candidates) == 1
    dispositions = product.reconciliation.for_family(FamilyId.PAIRED_RAMP_STEPS)
    assert [item.outcome for item in dispositions] == [Outcome.ACCEPTED]
    assert len(product.evidence.defining_of(candidate_set.candidates[0])) == 3
    assert product.result.paired_ramp_steps == tuple(recognise_paired_ramp_steps(part))
    assert build_recognition_result(part).paired_ramp_steps == product.result.paired_ramp_steps
    assert feature_census(part)["paired_ramp_step"] == 1


def test_a_blind_v_recess_is_not_a_through_side_step() -> None:
    assert recognise_paired_ramp_steps(_side_cut(blind=True)) == []


def test_a_top_opening_triangular_pocket_is_not_a_side_step() -> None:
    assert recognise_paired_ramp_steps(pocket_fixture()) == []


def test_candidate_refuses_incomplete_direction_terminal_arc_and_span_proofs(monkeypatch) -> None:
    graph, left, right, left_read, right_read = _proved_pair()

    with monkeypatch.context() as patch:
        patch.setattr(paired_ramp_module, "_RUN_DIRECTION_COS", 1.1)
        assert paired_ramp_module._candidate(graph, left, right, left_read, right_read) is None
    with monkeypatch.context() as patch:
        patch.setattr(graph, "neighbours", lambda _node: ())
        assert paired_ramp_module._candidate(graph, left, right, left_read, right_read) is None
    with monkeypatch.context() as patch:
        patch.setattr(graph, "is_planar", lambda _node: False)
        assert paired_ramp_module._candidate(graph, left, right, left_read, right_read) is None
    with monkeypatch.context() as patch:
        patch.setattr(paired_ramp_module, "_is_convex", lambda *_args: False)
        assert paired_ramp_module._candidate(graph, left, right, left_read, right_read) is None
    with monkeypatch.context() as patch:
        shared = graph.shared_edges(left, right)
        patch.setattr(graph, "shared_edges", lambda *_args: shared + shared)
        assert paired_ramp_module._candidate(graph, left, right, left_read, right_read) is None

    axis, normal, spans, hi, lo = left_read
    mismatched = dict(spans)
    mismatched[axis] = (spans[axis][0] + 1.0, spans[axis][1])
    assert (
        paired_ramp_module._candidate(
            graph, left, right, (axis, normal, mismatched, hi, lo), right_read
        )
        is None
    )


def test_an_asymmetric_v_is_outside_the_first_supported_domain() -> None:
    assert recognise_paired_ramp_steps(_side_cut(asymmetric=True)) == []


def test_one_or_two_unrelated_chamfers_do_not_form_a_paired_cut() -> None:
    box = Box(40, 40, 30)
    vertical = box.edges().filter_by(Axis.Z)

    assert recognise_paired_ramp_steps(chamfer(vertical[0], 3)) == []
    assert recognise_paired_ramp_steps(chamfer([vertical[0], vertical[2]], 3)) == []


def test_every_principal_run_axis_is_the_same_geometry_under_permutation() -> None:
    y_step = recognise_paired_ramp_steps(_side_cut())[0]
    x_step = recognise_paired_ramp_steps(Rot(0, 0, 90) * _side_cut())[0]
    z_step = recognise_paired_ramp_steps(Rot(90, 0, 0) * _side_cut())[0]

    assert (x_step.axis, y_step.axis, z_step.axis) == ("x", "y", "z")
    assert {x_step.angle, y_step.angle, z_step.angle} == {51.34}
    assert {x_step.length, y_step.length, z_step.length} == {25.0}


def test_translation_moves_only_the_stable_shared_ridge_anchor() -> None:
    assert recognise_paired_ramp_steps(Pos(3, 4, 5) * _side_cut()) == [
        PairedRampStep(axis="y", angle=51.34, length=25.0, at=(13.0, 11.5, 5.0))
    ]


def test_profile_traversal_start_does_not_change_the_record() -> None:
    assert recognise_paired_ramp_steps(_side_cut(cycle=1)) == recognise_paired_ramp_steps(
        _side_cut(cycle=2)
    )


def test_nonprincipal_run_is_outside_the_supported_domain() -> None:
    assert recognise_paired_ramp_steps(Rot(0, 0, 17) * _side_cut()) == []


def test_an_added_material_rib_is_not_a_removed_ramp_pair() -> None:
    rib = Box(40, 40, 30) + Pos(20, 20, 0) * extrude(
        Plane.XZ * Polygon((0, -8), (0, 8), (10, 0)), 25
    )

    assert recognise_paired_ramp_steps(rib) == []


def test_an_open_shell_cannot_supply_same_valid_solid_authority() -> None:
    assert recognise_paired_ramp_steps(Shell(list(_side_cut().faces()))) == []


def test_two_solids_are_scoped_independently_without_cross_body_pairing() -> None:
    compound = Compound([_side_cut(), Pos(100, 0, 0) * _side_cut()])

    assert len(recognise_paired_ramp_steps(compound)) == 2


def test_a_terminal_interrupted_by_a_drilled_hole_retains_the_proved_pair() -> None:
    interrupted = _side_cut() - Pos(15, -5, 0) * Rot(90, 0, 0) * Cylinder(1, 6)

    assert recognise_paired_ramp_steps(interrupted) == [
        PairedRampStep(axis="y", angle=51.34, length=25.0, at=(10.0, 7.5, 0.0))
    ]


def test_a_straight_terminal_boundary_subdivision_retains_the_proved_pair() -> None:
    subdivided = _side_cut() - Pos(15, -1, 0) * Box(1, 2, 3)

    assert recognise_paired_ramp_steps(subdivided) == [
        PairedRampStep(axis="y", angle=51.34, length=25.0, at=(10.0, 7.5, 0.0))
    ]


def test_a_straight_ramp_boundary_subdivision_retains_the_original_face_pair() -> None:
    one_subdivided = _side_cut() - _ramp_boundary_notch()
    both_subdivided = one_subdivided - _ramp_boundary_notch(upper=True, y=10)

    for part, expected_edge_counts in (
        (one_subdivided, [4, 8]),
        (both_subdivided, [8, 8]),
    ):
        graph, left, right, _left_read, _right_read = _proved_pair_from(part)
        assert sorted((len(graph.edges(left)), len(graph.edges(right)))) == expected_edge_counts
        assert recognise_paired_ramp_steps(part) == [
            PairedRampStep(axis="y", angle=51.34, length=25.0, at=(10.0, 7.5, 0.0))
        ]


def test_an_independent_circular_ramp_inner_wire_retains_the_original_face_pair() -> None:
    interrupted = _side_cut() - _ramp_inner_wire()
    graph, left, right, _left_read, _right_read = _proved_pair_from(interrupted)

    ramp_faces = (graph.face(left), graph.face(right))
    assert sorted(len(face.inner_wires()) for face in ramp_faces) == [0, 1]
    assert recognise_paired_ramp_steps(interrupted) == [
        PairedRampStep(axis="y", angle=51.34, length=25.0, at=(10.0, 7.5, 0.0))
    ]


def test_multiple_coplanar_ramp_faces_are_not_traversed_or_merged() -> None:
    part = _side_cut()
    lower_ramp = next(
        face for face in part.faces() if (normal := face.normal_at()).X > 0.6 and normal.Z > 0.7
    )
    splitter = BRepFeat_SplitShape(part.wrapped)
    splitter.Add(
        Edge.make_line((10, 5, 0), (20, 5, -8)).wrapped,
        lower_ramp.wrapped,
    )
    splitter.Build()
    assert splitter.IsDone()
    split = Solid(splitter.Shape())
    assert split.is_valid

    graph = FaceGraph(split)
    ramps = [
        node
        for node in graph.nodes
        if (normal := graph.normal(node)) is not None and normal[0] > 0.6 and abs(normal[2]) > 0.7
    ]
    assert len(ramps) == 3
    assert (
        sum(
            graph.normal(left) == graph.normal(right) and bool(graph.shared_edges(left, right))
            for index, left in enumerate(ramps)
            for right in ramps[index + 1 :]
        )
        == 1
    )

    ledger = ClaimLedger(graph)
    assert recognise_paired_ramp_steps(split, ledger=ledger) == []
    assert ledger.claims == ()


def test_subdivided_ramps_remain_covariant_and_profile_order_independent() -> None:
    part = _side_cut() - _ramp_boundary_notch()

    assert recognise_paired_ramp_steps(Rot(0, 0, 90) * part)[0].axis == "x"
    assert recognise_paired_ramp_steps(Rot(90, 0, 0) * part)[0].axis == "z"
    assert recognise_paired_ramp_steps(Pos(3, 4, 5) * part) == [
        PairedRampStep(axis="y", angle=51.34, length=25.0, at=(13.0, 11.5, 5.0))
    ]
    assert recognise_paired_ramp_steps(_side_cut(cycle=1) - _ramp_boundary_notch()) == (
        recognise_paired_ramp_steps(_side_cut(cycle=2) - _ramp_boundary_notch())
    )


def test_interrupted_ramp_recognition_is_scale_independent() -> None:
    small = recognise_paired_ramp_steps(_side_cut(scale=0.01) - _ramp_inner_wire(scale=0.01))[0]
    large = recognise_paired_ramp_steps(_side_cut(scale=100.0) - _ramp_inner_wire(scale=100.0))[0]

    assert small.axis == large.axis == "y"
    assert small.angle == large.angle == 51.34
    assert small.length == 0.25
    assert large.length == 2500.0


def test_subdivided_ramp_survives_step_round_trip(tmp_path: Path) -> None:
    part = _side_cut() - _ramp_boundary_notch()
    path = tmp_path / "subdivided-paired-ramp.step"

    assert export_step(part, path)
    assert recognise_paired_ramp_steps(import_step(path)) == recognise_paired_ramp_steps(part)


def test_two_boundary_subdivided_occurrences_retain_distinct_original_evidence() -> None:
    first = _side_cut() - _ramp_boundary_notch()
    second = Pos(100, 0, 0) * (_side_cut() - _ramp_inner_wire(upper=True))
    product = _take_inventory(Compound([first, second]))
    accepted = [
        item
        for item in product.reconciliation.for_family(FamilyId.PAIRED_RAMP_STEPS)
        if item.outcome is Outcome.ACCEPTED
    ]

    assert [item.candidate.record.at for item in accepted] == [
        (10.0, 7.5, 0.0),
        (110.0, 7.5, 0.0),
    ]
    defining = [product.evidence.defining_of(item.candidate) for item in accepted]
    assert [len(nodes) for nodes in defining] == [3, 3]
    assert defining[0].isdisjoint(defining[1])


def test_two_pairs_on_one_solid_keep_distinct_terminal_evidence() -> None:
    product = _take_inventory(_two_side_cuts())
    accepted = [
        item
        for item in product.reconciliation.for_family(FamilyId.PAIRED_RAMP_STEPS)
        if item.outcome is Outcome.ACCEPTED
    ]

    assert [item.candidate.record.at for item in accepted] == [
        (2.0, 7.5, 0.0),
        (22.0, 7.5, 0.0),
    ]
    defining = [product.evidence.defining_of(item.candidate) for item in accepted]
    assert [len(nodes) for nodes in defining] == [3, 3]
    assert defining[0].isdisjoint(defining[1])


def test_one_native_and_one_interrupted_pair_survive_on_the_same_solid() -> None:
    part = _two_side_cuts() - Pos(5, -5, 0) * Rot(90, 0, 0) * Cylinder(1, 6)
    product = _take_inventory(part)
    accepted = [
        item
        for item in product.reconciliation.for_family(FamilyId.PAIRED_RAMP_STEPS)
        if item.outcome is Outcome.ACCEPTED
    ]

    assert [item.candidate.record.at for item in accepted] == [
        (2.0, 7.5, 0.0),
        (22.0, 7.5, 0.0),
    ]
    terminal_edge_counts = sorted(
        len(product.context.graph.edges(node))
        for item in accepted
        for node in product.evidence.defining_of(item.candidate)
        if product.context.graph.normal(node) is not None
        and abs(product.context.graph.normal(node)[1]) >= 0.99
    )
    assert terminal_edge_counts == [3, 4]


def test_recognition_is_scale_independent_and_dimensions_scale() -> None:
    small = recognise_paired_ramp_steps(_side_cut(scale=0.01))[0]
    large = recognise_paired_ramp_steps(_side_cut(scale=100.0))[0]

    assert small.axis == large.axis == "y"
    assert small.angle == large.angle == 51.34
    assert small.length == 0.25
    assert large.length == 2500.0


def test_record_supports_a_concrete_paired_angle_and_run_dimension_projection() -> None:
    step = recognise_paired_ramp_steps(_side_cut())[0]

    assert {
        "leader": step.at,
        "paired_angles": (step.angle, step.angle),
        "run": (step.axis, step.length),
    } == {
        "leader": (10.0, 7.5, 0.0),
        "paired_angles": (51.34, 51.34),
        "run": ("y", 25.0),
    }
