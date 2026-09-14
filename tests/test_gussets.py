"""Material-positive gusset recognition and its aggregate contract."""

from __future__ import annotations

from dataclasses import replace

from build123d import (
    Align,
    Box,
    Compound,
    Location,
    Plane,
    Polygon,
    Pos,
    Rot,
    export_step,
    extrude,
    fillet,
)

from quiddity import (
    GussetRib,
    GussetRibArray,
    GussetRibMirrorPair,
    build_framed_recognition_report,
    build_raw_recognition_result,
    import_step_geometry,
    recognise_gusset_rib_patterns,
    recognise_gusset_ribs,
)
from quiddity._adjacency import FaceGraph, edge_face_map
from quiddity._body_identity import body_signature
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity.gussets import _cap
from tests.golden.gusset_rib_patterns.fixture import build_fixture as build_pattern_fixture
from tests.golden.gusset_ribs.fixture import build_fixture


def _plain_rib(x: float = -32):
    plate = Box(80, 50, 8, align=(Align.CENTER, Align.CENTER, Align.MIN))
    flange = Box(80, 8, 48, align=(Align.CENTER, Align.CENTER, Align.MIN)).moved(
        Location((0, 21, 0))
    )
    triangle = extrude(Plane.YZ * Polygon((17, 8), (-5, 8), (17, 34), align=None), amount=6)
    return plate + flange + triangle.moved(Location((x, 0, 0)))


def test_issue_fixture_reports_both_ribs_and_retains_other_features(tmp_path):
    part = build_fixture()
    body_key = body_signature(part.solids()[0])
    expected = (
        GussetRib("x", (-38.0, -32.0), (("y", 17.0), ("z", 8.0)), (22.0, 26.0), (-1, 1), body_key),
        GussetRib("x", (20.0, 26.0), (("y", 17.0), ("z", 8.0)), (22.0, 26.0), (-1, 1), body_key),
    )
    assert tuple(recognise_gusset_ribs(part)) == expected
    assert recognise_gusset_rib_patterns(expected) == []
    result = build_raw_recognition_result(part)
    assert result.gusset_ribs == expected
    assert result.gusset_rib_patterns == ()
    assert (len(result.holes), len(result.bosses), len(result.slots)) == (3, 1, 1)
    framed = build_framed_recognition_report(part)
    family = next(item for item in framed.report.families if item.family == "gusset_ribs")
    assert (family.proposed, family.accepted, family.rejected) == (2, 2, 0)
    assert len(framed.report.result.gusset_ribs) == 2
    # A consumer can reconstruct each end-cap centre in the paired caller-space frame.
    for rib, expected_rib in zip(framed.report.result.gusset_ribs, expected, strict=True):
        for local_bound, world_bound in zip(
            rib.thickness_bounds, expected_rib.thickness_bounds, strict=True
        ):
            coordinates = [0.0, 0.0, 0.0]
            coordinates["xyz".index(rib.thickness_axis)] = local_bound
            for (axis, support), leg, direction in zip(
                rib.supports, rib.legs, rib.directions, strict=True
            ):
                coordinates["xyz".index(axis)] = support + direction * leg / 3
            world = framed.frame.to_world(tuple(coordinates))
            assert abs(world[0] - world_bound) < 0.002
            assert abs(world[1] - (17 - 22 / 3)) < 0.002
            assert abs(world[2] - (8 + 26 / 3)) < 0.002

    path = tmp_path / "gusset-bracket.step"
    assert export_step(part, path)
    imported = import_step_geometry(path)
    assert tuple(recognise_gusset_ribs(imported)) == expected
    assert len(build_framed_recognition_report(imported).report.result.gusset_ribs) == 2


def test_one_rib_and_a_symmetric_pair_are_independent_occurrences():
    one = _plain_rib(-26)
    assert len(recognise_gusset_ribs(one)) == 1
    assert recognise_gusset_rib_patterns(recognise_gusset_ribs(one)) == []
    triangle = extrude(Plane.YZ * Polygon((17, 8), (-5, 8), (17, 34), align=None), amount=6)
    mirrored = one + triangle.moved(Location((32, 0, 0)))
    ribs = tuple(recognise_gusset_ribs(mirrored))
    assert tuple(record.thickness_bounds for record in ribs) == (
        (-32.0, -26.0),
        (26.0, 32.0),
    )
    pair = GussetRibMirrorPair(ribs, ("x", 0.0))
    assert recognise_gusset_rib_patterns(ribs) == [pair]
    assert build_raw_recognition_result(mirrored).gusset_rib_patterns == (pair,)


def test_three_matching_ribs_form_one_constant_pitch_array():
    plate = Box(80, 50, 8, align=(Align.CENTER, Align.CENTER, Align.MIN))
    flange = Box(80, 8, 48, align=(Align.CENTER, Align.CENTER, Align.MIN)).moved(
        Location((0, 21, 0))
    )
    triangle = extrude(Plane.YZ * Polygon((17, 8), (-5, 8), (17, 34), align=None), amount=6)
    part = plate + flange
    for x in (-20, 0, 20):
        part += triangle.moved(Location((x, 0, 0)))
    ribs = tuple(recognise_gusset_ribs(part))
    assert len(ribs) == 3
    array = GussetRibArray(ribs, "x", 20.0)
    assert recognise_gusset_rib_patterns(ribs) == [array]
    assert build_raw_recognition_result(part).gusset_rib_patterns == (array,)

    perturbed = plate + flange
    for x in (-20, 0, 20.02):
        perturbed += triangle.moved(Location((x, 0, 0)))
    (near_array,) = recognise_gusset_rib_patterns(recognise_gusset_ribs(perturbed))
    assert isinstance(near_array, GussetRibArray)
    assert near_array.pitch == 20.01


def test_mirror_pair_survives_framed_motion_and_step_round_trip(tmp_path):
    part = build_pattern_fixture()
    rounded_edges = [edge for edge in part.edges() if abs(edge.length - 34.05877) < 0.01]
    rounded = fillet(rounded_edges, radius=1)
    assert len(rounded_edges) == 4
    assert [
        type(pattern) for pattern in recognise_gusset_rib_patterns(recognise_gusset_ribs(rounded))
    ] == [GussetRibMirrorPair]

    rotated = Pos(4, 7, -2) * Rot(11, 23, 7) * part
    report = build_framed_recognition_report(rotated)
    (pair,) = report.report.result.gusset_rib_patterns
    assert isinstance(pair, GussetRibMirrorPair)
    assert pair.ribs == report.report.result.gusset_ribs

    path = tmp_path / "symmetric-gussets.step"
    assert export_step(part, path)
    imported = import_step_geometry(path)
    assert tuple(recognise_gusset_rib_patterns(recognise_gusset_ribs(imported))) == (
        GussetRibMirrorPair(tuple(recognise_gusset_ribs(imported)), ("x", 0.0)),
    )


def test_uneven_or_cross_body_ribs_do_not_form_a_pattern():
    plate = Box(80, 50, 8, align=(Align.CENTER, Align.CENTER, Align.MIN))
    flange = Box(80, 8, 48, align=(Align.CENTER, Align.CENTER, Align.MIN)).moved(
        Location((0, 21, 0))
    )
    triangle = extrude(Plane.YZ * Polygon((17, 8), (-5, 8), (17, 34), align=None), amount=6)
    uneven = plate + flange
    for x in (-20, 0, 23):
        uneven += triangle.moved(Location((x, 0, 0)))
    assert recognise_gusset_rib_patterns(recognise_gusset_ribs(uneven)) == []

    separated = Compound([_plain_rib(-26), Pos(100, 0, 0) * _plain_rib(-26)])
    assert len(recognise_gusset_ribs(separated)) == 2
    assert recognise_gusset_rib_patterns(recognise_gusset_ribs(separated)) == []

    # The serialized body key is a correlation value, not permission to pair ambiguous bodies.
    ambiguous = [replace(rib, body_key=None) for rib in recognise_gusset_ribs(separated)]
    assert recognise_gusset_rib_patterns(ambiguous) == []

    coincident = recognise_gusset_ribs(Compound([_plain_rib(-26), _plain_rib(-26)]))
    assert len(coincident) == 2
    assert all(rib.body_key is None for rib in coincident)
    assert recognise_gusset_rib_patterns(coincident) == []


def test_framed_rigid_motion_preserves_both_occurrences():
    moved = Pos(4, 7, -2) * Rot(11, 23, 7) * build_fixture()
    framed = build_framed_recognition_report(moved)
    assert len(framed.report.result.gusset_ribs) == 2


def test_a_triangular_void_is_not_a_material_rib():
    stock = Box(80, 50, 48, align=(Align.CENTER, Align.CENTER, Align.MIN))
    triangle = extrude(Plane.YZ * Polygon((17, 8), (-5, 8), (17, 34), align=None), amount=6)
    assert recognise_gusset_ribs(stock - triangle.moved(Location((-32, 0, 0)))) == []


def test_a_hollow_rib_fails_the_material_proof_after_its_caps_match():
    hollow = _plain_rib() - Box(2, 2, 2).moved(Location((-35, 5, 14)))
    graph = FaceGraph(hollow)
    incident = edge_face_map(hollow.faces())
    assert sum(_cap(face, incident, graph, None) is not None for face in hollow.faces()) == 2
    assert recognise_gusset_ribs(hollow) == []


def test_touching_but_unfused_solids_do_not_form_a_rib():
    plate = Box(80, 50, 8, align=(Align.CENTER, Align.CENTER, Align.MIN))
    flange = Box(80, 8, 48, align=(Align.CENTER, Align.CENTER, Align.MIN)).moved(
        Location((0, 21, 0))
    )
    triangle = extrude(Plane.YZ * Polygon((17, 8), (-5, 8), (17, 34), align=None), amount=6)
    assembly = Compound([plate + flange, triangle.moved(Location((-32, 0, 0)))])
    assert len(assembly.solids()) == 2
    assert recognise_gusset_ribs(assembly) == []


def test_signed_legs_locate_a_rib_on_the_other_side_of_a_flange():
    plate = Box(80, 50, 8, align=(Align.CENTER, Align.CENTER, Align.MIN))
    flange = Box(80, 8, 48, align=(Align.CENTER, Align.CENTER, Align.MIN)).moved(
        Location((0, -21, 0))
    )
    triangle = extrude(Plane.YZ * Polygon((-17, 8), (5, 8), (-17, 34), align=None), amount=6)
    (rib,) = recognise_gusset_ribs(plate + flange + triangle.moved(Location((-32, 0, 0))))
    assert rib.supports == (("y", -17.0), ("z", 8.0))
    assert rib.legs == (22.0, 26.0)
    assert rib.directions == (1, 1)


def test_a_rounded_hypotenuse_keeps_the_virtual_leg_dimensions(tmp_path):
    sharp = _plain_rib()
    hypotenuse_edges = [edge for edge in sharp.edges() if abs(edge.length - 34.05877) < 0.01]
    for selection, expected_faces in ((hypotenuse_edges[:1], 4), (hypotenuse_edges, 5)):
        rounded = fillet(selection, radius=1)
        assert [replace(rib, body_key=None) for rib in recognise_gusset_ribs(rounded)] == [
            replace(rib, body_key=None) for rib in recognise_gusset_ribs(sharp)
        ]
        ledger = ClaimLedger(FaceGraph(rounded))
        (record,) = recognise_gusset_ribs(rounded, ledger=ledger)
        (candidate,) = ledger.candidate_set_for(FamilyId.GUSSET_RIBS, [record]).candidates
        assert len(ledger.defining_of(candidate)) == expected_faces
    path = tmp_path / "rounded-rib.step"
    assert export_step(rounded, path)
    assert [
        replace(rib, body_key=None) for rib in recognise_gusset_ribs(import_step_geometry(path))
    ] == [replace(rib, body_key=None) for rib in recognise_gusset_ribs(sharp)]


def test_evidence_owns_original_rib_faces_and_step_round_trip(tmp_path):
    part = _plain_rib()
    ledger = ClaimLedger(FaceGraph(part))
    (record,) = recognise_gusset_ribs(part, ledger=ledger)
    (candidate,) = ledger.candidate_set_for(FamilyId.GUSSET_RIBS, [record]).candidates
    assert len(ledger.defining_of(candidate)) == 3
    assert ledger.graph.common_valid_solid(ledger.defining_of(candidate)) is not None

    path = tmp_path / "one-rib.step"
    assert export_step(part, path)
    imported = import_step_geometry(path)
    assert recognise_gusset_ribs(imported) == [record]
