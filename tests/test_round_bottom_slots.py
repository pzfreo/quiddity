# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Exact analytic U-section blind-slot recognition."""

from __future__ import annotations

import pytest
from build123d import (
    Axis,
    Box,
    BuildLine,
    BuildSketch,
    Compound,
    Cylinder,
    GeomType,
    Keep,
    Line,
    Plane,
    Pos,
    RadiusArc,
    Rot,
    Shell,
    Solid,
    Spline,
    Vector,
    export_step,
    extrude,
    import_step,
    make_face,
)

import quiddity.round_bottom_slots as round_bottom_slots
from quiddity._adjacency import FaceGraph
from quiddity._claims import ClaimLedger
from quiddity.evidence import build_recognition_evidence
from quiddity.frames import FramedRecognitionResult, build_framed_recognition_result
from quiddity.round_bottom_slots import (
    RoundBottomBlindSlot,
    _alternating_profile_runs,
    _boundary_runs,
    _common_convex_context,
    _coplanar_region,
    _Cylinder,
    _cylinder_surface,
    _empty_sweep,
    _length_tolerance,
    _principal_rectangle,
    _quarter_cylinder,
    _region_boundary_wire,
    _region_face,
    _same_cylinder,
    _same_span,
    recognise_round_bottom_blind_slots,
)
from tools._legacy_recognition import (
    build_raw_recognition_result,
    build_recognition_result,
)


def _profile(width: float, radius: float):
    flat = width - 2 * radius
    half_width = width / 2
    half_flat = flat / 2
    with BuildLine() as boundary:
        Line((-half_width, 0), (half_width, 0))
        RadiusArc((half_width, 0), (half_flat, -radius), radius)
        Line((half_flat, -radius), (-half_flat, -radius))
        RadiusArc((-half_flat, -radius), (-half_width, 0), radius)
    with BuildSketch() as sketch:
        make_face(boundary.line)
    return sketch.sketch


def _slot(scale: float = 1.0):
    width, radius, length = 10 * scale, 3 * scale, 20 * scale
    stock = Pos(0, -5 * scale, 0) * Box(30 * scale, 10 * scale, 40 * scale)
    tool = extrude(_profile(width, radius), amount=length, dir=Vector(0, 0, 1))
    return stock - tool


def _split_faces(part, predicate, plane) -> Solid:
    faces = []
    for face in part.faces():
        if predicate(face):
            top, bottom = face.split(plane, Keep.BOTH)
            faces.extend((top, bottom))
        else:
            faces.append(face)
    rebuilt = Solid(Shell(faces))
    assert rebuilt.is_valid
    return rebuilt


@pytest.mark.parametrize("rotation", [Rot(0, 0, 0), Rot(90, 0, 0), Rot(0, 90, 0), Rot(180, 0, 0)])
def test_unified_slot_preserves_world_placement_and_ends(rotation):
    result = build_raw_recognition_result(Pos(123, -57, 91) * rotation * _slot())
    (source,) = result.round_bottom_blind_slots
    (unified,) = result.section_recesses
    geometry = unified.geometry
    frame = geometry.frame
    midpoint = sum(geometry.run_interval) / 2
    center = tuple(frame.origin[index] + midpoint * frame.run[index] for index in range(3))
    assert center == pytest.approx(source.at, abs=0.002)
    assert geometry.ends.low.condition == ("open" if source.open_sign == -1 else "capped")
    assert geometry.ends.high.condition == ("open" if source.open_sign == 1 else "capped")


def test_round_bottom_blind_slot_has_truthful_dimensions_and_evidence():
    part = _slot()
    ledger = ClaimLedger(FaceGraph(part))

    actual = recognise_round_bottom_blind_slots(part, ledger=ledger)
    assert actual == [
        RoundBottomBlindSlot(
            axis="z",
            open_sign=1,
            length=20.0,
            width_axis="x",
            depth_axis="y",
            depth_sign=1,
            radius=3.0,
            flat_width=4.0,
            at=(0.0, -1.5, 10.0),
        )
    ]
    assert actual[0].width == 10.0
    assert actual[0].depth == 3.0
    assert len(ledger.claims) == 1
    assert len(ledger.claims[0].defining) == 4

    evidence = build_recognition_evidence(part)
    (feature,) = tuple(
        ref for ref in evidence.features if evidence.family(ref) == "section_recesses"
    )
    assert evidence.constituent_faces(feature) == evidence.defining_faces(feature)
    assert len(evidence.defining_faces(feature)) == 4

    (unified,) = build_raw_recognition_result(part).section_recesses
    assert unified.classification.feature_kind == "edge_open_recess"
    assert unified.classification.section_shape == "general"
    assert unified.geometry.profile.closure == "open"
    assert [vertex.bulge != 0.0 for vertex in unified.geometry.profile.boundary] == [
        True,
        False,
        True,
        False,
    ]


def test_axis_open_sign_scale_and_step_roundtrip_are_stable(tmp_path):
    base = _slot()
    path = tmp_path / "round-bottom-slot.step"
    export_step(base, path)
    assert recognise_round_bottom_blind_slots(
        import_step(path)
    ) == recognise_round_bottom_blind_slots(base)

    for part, axis, sign in (
        (base, "z", 1),
        (Rot(180, 0, 0) * base, "z", -1),
        (Rot(90, 0, 0) * base, "y", -1),
        (Rot(0, 90, 0) * base, "x", 1),
    ):
        assert [
            (record.axis, record.open_sign) for record in recognise_round_bottom_blind_slots(part)
        ] == [(axis, sign)]

    for scale in (0.001, 1000.0):
        (record,) = recognise_round_bottom_blind_slots(_slot(scale))
        assert record.length == pytest.approx(20 * scale, abs=max(0.001, scale * 1e-6))
        assert record.radius == pytest.approx(3 * scale, abs=max(0.001, scale * 1e-6))


def test_local_cylinder_equality_accepts_and_rejects_both_sides_of_the_tolerance():
    radius = 3.0
    tolerance = _length_tolerance(radius)
    reference = (radius, 2, (0.0, 0.0, 0.0))

    assert _same_cylinder(reference, (radius + 0.5 * tolerance, 2, (0.0, 0.0, 1.0)))
    assert not _same_cylinder(reference, (radius + 2 * tolerance, 2, (0.0, 0.0, 1.0)))
    assert not _same_cylinder(reference, (radius, 2, (2 * tolerance, 0.0, 1.0)))
    translated = (radius, 2, (1e9, 1e9, 0.0))
    assert _same_cylinder(
        translated,
        (radius, 2, (1e9 + 0.5 * tolerance, 1e9, 1.0)),
    )
    assert not _same_cylinder(
        translated,
        (radius, 2, (1e9 + 2 * tolerance, 1e9, 1.0)),
    )


@pytest.mark.parametrize("start", range(5))
def test_boundary_runs_join_split_straight_side_across_every_wire_seam(start):
    # Control enumeration, not geometry: OCCT may choose any edge as the wire seam.
    points = [(0, 0), (1, 0), (2, 0), (2, 2), (0, 2), (0, 0)]
    edges = [Line(first, second) for first, second in zip(points[:-1], points[1:], strict=True)]
    ordered = edges[start:] + edges[:start]

    class OrderedBoundary:
        @staticmethod
        def edges():
            return ordered

    groups = _boundary_runs(OrderedBoundary())
    assert groups is not None
    assert len(groups) == 4
    assert all(kind == GeomType.LINE for kind, _members in groups)
    assert sorted(len(members) for _kind, members in groups) == [1, 1, 1, 2]
    (split_side,) = [members for _kind, members in groups if len(members) == 2]
    assert split_side[0] is edges[0]
    assert split_side[1] is edges[1]
    assert sorted(id(edge) for _kind, members in groups for edge in members) == sorted(
        id(edge) for edge in edges
    )


def test_helper_refusal_boundaries_are_total_and_fail_closed(monkeypatch):
    graph = FaceGraph(_slot())
    assert _region_boundary_wire(graph, frozenset()) is None
    assert _region_face(graph, frozenset()) is None
    assert not _principal_rectangle(graph, frozenset(), 1)
    assert not _quarter_cylinder(graph, _Cylinder(frozenset(), 3.0, 2, (0.0, 0.0, 0.0)), (0, 20))
    assert not _common_convex_context(graph, (), 2, 0.0, 20.0)

    shared_edge = object()

    class NonManifoldGraph:
        @staticmethod
        def face(_node):
            return type("ValidFace", (), {"is_valid": True})()

        @staticmethod
        def edges(_node):
            return (shared_edge,)

    assert _region_boundary_wire(NonManifoldGraph(), frozenset({1, 2, 3})) is None

    with BuildLine() as spline_boundary:
        Spline((0, 0), (1, 1), (2, 0))
    assert _boundary_runs(spline_boundary.line) is None
    assert _alternating_profile_runs(spline_boundary.line) is None

    class BoundsGraph:
        @staticmethod
        def bounds(node):
            return ((0.0, 1.0), (0.0, 1.0), (0.0, float(node)))

    regions = (frozenset({1}), frozenset({2}), frozenset({1}))
    assert _same_span(BoundsGraph(), regions, 2) is None

    class EmptyIntersection:
        @staticmethod
        def intersect(_probe):
            return None

    cap = _profile(10, 3).faces()[0]
    assert _empty_sweep(cap, EmptyIntersection(), 2, 20)

    class ShapeListIntersection:
        @staticmethod
        def intersect(_probe):
            return [type("ZeroVolume", (), {"volume": 0.0})()]

    assert _empty_sweep(cap, ShapeListIntersection(), 2, 20)

    class ShapeIntersection:
        @staticmethod
        def intersect(_probe):
            return type("ZeroVolume", (), {"volume": 0.0})()

    assert _empty_sweep(cap, ShapeIntersection(), 2, 20)

    presented = _slot().rotate(Axis((0, 0, 0), (1, 1, 0)), 37)
    presented_graph = FaceGraph(presented)
    curved = next(
        node
        for node in presented_graph.nodes
        if presented_graph.face(node).geom_type == GeomType.CYLINDER
    )
    planar = next(
        node
        for node in presented_graph.nodes
        if presented_graph.face(node).geom_type == GeomType.PLANE
    )
    assert _cylinder_surface(presented_graph, curved) is None
    assert _coplanar_region(presented_graph, planar) == frozenset()

    cylinder = Cylinder(5, 10)
    cylinder_graph = FaceGraph(cylinder)
    circular_end = next(
        node
        for node in cylinder_graph.nodes
        if cylinder_graph.face(node).geom_type == GeomType.PLANE
    )
    assert not _principal_rectangle(cylinder_graph, frozenset({circular_end}), 2)

    rectangular_wire = Box(1, 1, 1).faces()[0].outer_wire()
    assert _boundary_runs(rectangular_wire) is not None
    assert _alternating_profile_runs(rectangular_wire) is None

    with monkeypatch.context() as context:
        context.setattr(round_bottom_slots, "_same_span", lambda *_args: None)
        assert recognise_round_bottom_blind_slots(_slot()) == []
    with monkeypatch.context() as context:
        context.setattr(round_bottom_slots, "_region_face", lambda *_args: None)
        assert recognise_round_bottom_blind_slots(_slot()) == []


def test_translation_mirror_and_arbitrary_framed_presentation_preserve_the_feature():
    base = _slot()
    (translated,) = recognise_round_bottom_blind_slots(Pos(17, -23, 9) * base)
    assert translated.at == pytest.approx((17.0, -24.5, 19.0), abs=1e-3)
    assert (translated.axis, translated.open_sign) == ("z", 1)

    (mirrored,) = recognise_round_bottom_blind_slots(base.mirror(Plane.XY))
    assert (mirrored.axis, mirrored.open_sign) == ("z", -1)
    assert (mirrored.radius, mirrored.flat_width, mirrored.length) == (3.0, 4.0, 20.0)

    presented = Pos(-31, 17, 23) * base.rotate(Axis((0, 0, 0), (1, 1, 0)), 37)
    assert recognise_round_bottom_blind_slots(presented) == []
    framed = build_framed_recognition_result(presented, rotational=False)
    assert isinstance(framed, FramedRecognitionResult)
    (record,) = framed.result.section_recesses
    assert record.classification.feature_kind == "edge_open_recess"
    assert record.geometry.profile.closure == "open"
    assert record.geometry.run_interval[1] - record.geometry.run_interval[0] == pytest.approx(20)
    assert sum(vertex.bulge != 0 for vertex in record.geometry.profile.boundary) == 2


def test_opposite_depth_openings_have_distinct_public_records():
    base = _slot()
    opposite = Pos(0, -3, 0) * base.mirror(Plane.XZ)

    (positive,) = recognise_round_bottom_blind_slots(base)
    (negative,) = recognise_round_bottom_blind_slots(opposite)

    assert positive.depth_sign == 1
    assert negative.depth_sign == -1
    assert positive != negative


def test_cap_side_and_context_subdivisions_preserve_the_logical_feature():
    base = _slot()
    expected = recognise_round_bottom_blind_slots(base)
    cap_split = _split_faces(
        base,
        lambda face: abs(face.center().Z) < 1e-8 and face.area < 30,
        Plane.YZ,
    )
    side_split = _split_faces(
        base,
        lambda face: (
            face.geom_type == GeomType.CYLINDER
            or (abs(face.center().Y + 3) < 1e-7 and abs(face.center().Z - 10) < 1e-7)
        ),
        Plane.XY.offset(10),
    )
    mouth_split = _split_faces(
        base,
        lambda face: abs(face.center().Z - 20) < 1e-7,
        Plane.YZ,
    )

    for part in (cap_split, side_split, mouth_split):
        ledger = ClaimLedger(FaceGraph(part))
        assert recognise_round_bottom_blind_slots(part, ledger=ledger) == expected
        assert set(ledger.claims[0].defining) <= set(ledger.graph.nodes)


def test_cap_holes_and_stock_continuations_are_rejected():
    base = _slot()
    cap_hole = base - Pos(0, -1.5, -5) * Cylinder(0.5, 10)
    cap_breakout = base - Pos(0, -1.5, -5) * Box(2, 2, 10)

    assert recognise_round_bottom_blind_slots(cap_hole) == []
    assert recognise_round_bottom_blind_slots(cap_breakout) == []


def test_floor_holes_and_boundary_notches_are_rejected():
    base = _slot()
    floor_hole = base - Pos(0, -3, 10) * Rot(90, 0, 0) * Cylinder(0.5, 7)
    floor_notch = base - Pos(0, -4, 10) * Box(1, 4, 2)

    assert recognise_round_bottom_blind_slots(floor_hole) == []
    assert recognise_round_bottom_blind_slots(floor_notch) == []


def test_through_and_doubly_capped_u_sections_are_rejected():
    stock = Pos(0, -5, 0) * Box(30, 10, 40)
    through = stock - Pos(0, 0, -20) * extrude(_profile(10, 3), amount=40, dir=Vector(0, 0, 1))
    sealed = stock - Pos(0, 0, -10) * extrude(_profile(10, 3), amount=20, dir=Vector(0, 0, 1))

    assert recognise_round_bottom_blind_slots(through) == []
    assert recognise_round_bottom_blind_slots(sealed) == []


def test_rectangular_notches_and_top_open_obround_pockets_are_other_families():
    stock = Pos(0, -5, 0) * Box(30, 10, 40)
    rectangular = stock - Pos(0, -1.5, 10) * Box(10, 3, 20)
    sealed_mouth = stock - Pos(0, -1, 0) * extrude(_profile(10, 3), amount=20, dir=Vector(0, 0, 1))

    assert recognise_round_bottom_blind_slots(rectangular) == []
    assert recognise_round_bottom_blind_slots(sealed_mouth) == []


def test_material_inside_the_claimed_sweep_rejects_the_candidate():
    interrupted = _slot() + Pos(0, -2, 10) * Box(1, 2, 10)
    assert recognise_round_bottom_blind_slots(interrupted) == []


def test_compound_members_and_equal_occurrences_remain_distinct():
    first = _slot()
    second = Pos(100, 0, 0) * _slot()
    part = Compound(children=[first, second])

    records = recognise_round_bottom_blind_slots(part)
    assert len(records) == 2
    assert records[0].at != records[1].at

    evidence = build_recognition_evidence(Compound(children=[_slot(), _slot()]))
    refs = tuple(ref for ref in evidence.features if evidence.family(ref) == "section_recesses")
    assert len(refs) == 2
    assert refs[0] is not refs[1]


def test_invalid_open_solid_and_mixed_compound_never_publish_invalid_evidence():
    valid = Pos(-100, 0, 0) * _slot()
    invalid_source = Pos(100, 0, 0) * _slot()
    removed = max(invalid_source.faces(), key=lambda face: face.area)
    invalid = Solid(Shell(face for face in invalid_source.faces() if not face.is_same(removed)))
    assert not invalid.is_valid

    assert recognise_round_bottom_blind_slots(invalid) == []

    mixed = Compound(children=[valid, invalid])
    ledger = ClaimLedger(FaceGraph(mixed))
    records = recognise_round_bottom_blind_slots(mixed, ledger=ledger)

    assert len(records) == 1
    assert records[0].at[0] < 0
    assert len(ledger.claims) == 1
    assert ledger.graph.common_valid_solid(ledger.claims[0].defining) is not None


def test_two_slots_on_one_body_are_distinct_and_do_not_overlap_recess_families():
    stock = Pos(0, -5, 0) * Box(50, 10, 40)
    tool = extrude(_profile(10, 3), amount=20, dir=Vector(0, 0, 1))
    part = stock - Pos(-13, 0, 0) * tool - Pos(13, 0, 0) * tool

    direct = recognise_round_bottom_blind_slots(part)
    aggregate = build_recognition_result(part)

    assert len(direct) == 2
    assert aggregate.round_bottom_blind_slots == tuple(direct)
    assert aggregate.slots == aggregate.pockets == aggregate.channels == ()
