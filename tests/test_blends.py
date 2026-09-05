# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Public Blend chain, provenance, transformation and reconciliation contracts."""

from __future__ import annotations

import math

import pytest
from build123d import (
    Axis,
    Box,
    BuildPart,
    BuildSketch,
    Compound,
    Cylinder,
    GeomType,
    Plane,
    Pos,
    Rot,
    SlotOverall,
    Torus,
    export_step,
    extrude,
    fillet,
)
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeEdge
from OCP.BRepFeat import BRepFeat_SplitShape
from OCP.GeomAbs import GeomAbs_Torus

from quiddity import (
    Blend,
    CircularBlendPath,
    FramedRecognitionResult,
    StraightBlendPath,
    build_framed_recognition_result,
    build_raw_recognition_result,
    feature_census,
    import_step_geometry,
    recognise_blends,
)
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._claims import ClaimLedger
from quiddity._dispositions import Outcome, ReasonCode
from quiddity._reconcile import reconcile_blend_candidates
from quiddity.evidence import build_recognition_evidence
from quiddity.result import _take_inventory


def _external(radius: float = 2.0):
    box = Box(40, 30, 20)
    return fillet(list(box.edges().filter_by(Axis.Z)), radius)


def _internal():
    pocket = Box(40, 40, 20) - Pos(0, 0, 5) * Box(20, 20, 10)
    bottom = [
        edge
        for edge in pocket.edges()
        if abs(edge.center().Z) < 1e-6 and abs(edge.center().X) <= 10 and abs(edge.center().Y) <= 10
    ]
    return fillet(bottom, 2)


def _circular_blind_step():
    stock = Box(40, 30, 20)
    removal = Pos(7.5, 15, 10) * Rot(0, 90, 0) * Cylinder(4, 25)
    return stock - removal


def _annular_boss():
    return (Box(40, 40, 10) + Pos(20, 20, 10) * Cylinder(10, 8)) - (
        Pos(20, 20, 0) * Cylinder(5, 18)
    )


def _obround_passage():
    with BuildPart() as tool:
        with BuildSketch():
            SlotOverall(30, 10)
        extrude(amount=20, both=True)
    return Box(50, 40, 20) - tool.part


def _turned_toroidal(radius: float = 0.2):
    shaft = Pos(0, 0, 20) * Cylinder(15, 40) + Pos(0, 0, 55) * Cylinder(8, 30)
    return fillet([edge for edge in shaft.edges() if edge.geom_type == GeomType.CIRCLE], radius)


def _internal_toroidal_bottom(radius: float = 1.0):
    pocket = Box(60, 60, 20) - Pos(0, 0, 8) * Cylinder(5, 12)
    bottom = min(pocket.edges().filter_by(GeomType.CIRCLE), key=lambda edge: edge.center().Z)
    return fillet(bottom, radius)


def _rounded_signature(part) -> list[tuple]:
    return [
        (
            record.radius,
            record.side,
            record.path.at,
            tuple(round(value, 9) for value in record.path.direction),
        )
        for record in recognise_blends(part)
        if isinstance(record.path, StraightBlendPath)
    ]


def test_direct_convex_chains_are_superseded_by_dimension_worthy_fillets() -> None:
    part = _external()
    direct = recognise_blends(part)
    product = _take_inventory(part)
    proposed = product.physical.candidate_set(FamilyId.BLENDS).candidates

    assert len(direct) == len(proposed) == 4
    assert all(record.side == "convex" for record in direct)
    assert product.result.blends == ()
    assert feature_census(part)["blend"] == 0
    decisions = product.reconciliation.for_family(FamilyId.BLENDS)
    assert len(decisions) == 4
    assert all(item.outcome is Outcome.REJECTED for item in decisions)
    assert all(item.reason is ReasonCode.BLEND_SUPERSEDED_BY_FILLET for item in decisions)
    assert all(item.related for item in decisions)


def test_only_an_accepted_fillet_can_supersede_a_blend() -> None:
    graph = FaceGraph(Box(10, 10, 10))
    ledger = ClaimLedger(graph)
    node = graph.nodes[0]
    blend = ledger.propose(FamilyId.BLENDS, object(), (node,))
    fillet = ledger.propose(FamilyId.FILLETS, object(), (node,))
    evidence = ledger.snapshot_index()
    blends = evidence.candidate_set(FamilyId.BLENDS)
    fillets = evidence.candidate_set(FamilyId.FILLETS)

    accepted_decisions = reconcile_blend_candidates(blends, fillets, evidence)
    rejected_decisions = reconcile_blend_candidates(
        blends,
        fillets,
        evidence,
        rejected_fillets=frozenset((fillet,)),
    )

    assert len(accepted_decisions) == 1
    assert accepted_decisions[0].candidate is blend
    assert accepted_decisions[0].reason is ReasonCode.BLEND_SUPERSEDED_BY_FILLET
    assert rejected_decisions == ()


def test_small_convex_chains_remain_public_with_exact_face_evidence() -> None:
    part = _external(0.2)
    view = build_recognition_evidence(part)

    assert len(view.result.blends) == 4
    assert feature_census(part)["blend"] == 4
    blend_refs = [feature for feature in view.features if view.family(feature) == "blends"]
    assert len(blend_refs) == 4
    for feature in blend_refs:
        assert isinstance(view.record(feature), Blend)
        assert len(view.defining_faces(feature)) == 1
        assert view.constituent_faces(feature) == view.defining_faces(feature)
        face = view.face(next(iter(view.defining_faces(feature))))
        assert face.geom_type.name == "CYLINDER"


def test_internal_rounds_are_public_concave_chains_with_exact_evidence() -> None:
    part = _internal()
    direct = recognise_blends(part)
    view = build_recognition_evidence(part)
    blend_refs = [feature for feature in view.features if view.family(feature) == "blends"]

    assert len(direct) == len(view.result.blends) == len(blend_refs) == 4
    assert all(record.side == "concave" and record.radius == 2 for record in direct)
    assert feature_census(part)["blend"] == 4
    for feature in blend_refs:
        assert view.constituent_faces(feature) == view.defining_faces(feature)
        assert len(view.defining_faces(feature)) == 1
        assert view.face(next(iter(view.defining_faces(feature)))).geom_type.name == "CYLINDER"

    pocket_ref = next(
        feature for feature in view.features if view.family(feature) == "section_recesses"
    )
    pocket_faces = view.constituent_faces(pocket_ref)
    assert all(view.constituent_faces(feature) <= pocket_faces for feature in blend_refs)

    product = _take_inventory(part)
    assert product.result.fillets == ()


def test_blend_side_rejects_values_without_proved_material_semantics() -> None:
    with pytest.raises(ValueError, match="convex or concave"):
        Blend(2, "neutral", StraightBlendPath((0, 0, 0), (0, 0, 1)))


def test_path_records_reject_invalid_geometry_and_canonicalise_directions() -> None:
    assert StraightBlendPath((1, 2, 3), (0, 0, -1)).direction == (0, 0, 1)
    assert CircularBlendPath((1, 2, 3), (0, -1, 0), 4).normal == (0, 1, 0)
    with pytest.raises(ValueError, match="anchor must be finite"):
        StraightBlendPath((math.nan, 0, 0), (0, 0, 1))
    with pytest.raises(ValueError, match="centre must be finite"):
        CircularBlendPath((0, math.inf, 0), (0, 0, 1), 4)
    with pytest.raises(ValueError, match="path radius must be positive"):
        CircularBlendPath((0, 0, 0), (0, 0, 1), 0)
    with pytest.raises(ValueError, match="blend radius must be positive"):
        Blend(0, "convex", StraightBlendPath((0, 0, 0), (0, 0, 1)))
    with pytest.raises(TypeError, match="straight or circular"):
        Blend(1, "convex", object())  # type: ignore[arg-type]


def test_native_toroidal_edges_publish_truthful_circular_paths_and_exact_faces() -> None:
    part = _turned_toroidal()
    direct = recognise_blends(part)
    view = build_recognition_evidence(part)
    refs = [feature for feature in view.features if view.family(feature) == "blends"]

    assert len(direct) == len(view.result.blends) == len(refs) == 4
    assert {record.side for record in direct} == {"convex", "concave"}
    assert all(record.radius == 0.2 for record in direct)
    assert all(isinstance(record.path, CircularBlendPath) for record in direct)
    assert {
        record.path.radius for record in direct if isinstance(record.path, CircularBlendPath)
    } == {
        7.8,
        8.2,
        14.8,
    }
    for feature in refs:
        defining = view.defining_faces(feature)
        assert defining == view.constituent_faces(feature)
        assert len(defining) == 1
        assert view.face(next(iter(defining))).geom_type == GeomType.TORUS


def test_internal_toroidal_bottom_is_a_concave_circular_blend() -> None:
    (record,) = recognise_blends(_internal_toroidal_bottom())

    assert record == Blend(
        1.0,
        "concave",
        CircularBlendPath(center=(0, 0, 3), normal=(0, 0, 1), radius=4),
    )


def test_dimension_worthy_turned_fillets_supersede_the_same_toroidal_blends() -> None:
    part = _turned_toroidal(0.8)
    product = _take_inventory(part)

    assert len(recognise_blends(part)) == 4
    assert len(product.result.fillets) == 4
    assert product.result.blends == ()
    decisions = product.reconciliation.for_family(FamilyId.BLENDS)
    assert len(decisions) == 4
    assert all(item.reason is ReasonCode.BLEND_SUPERSEDED_BY_FILLET for item in decisions)


def test_full_torus_bead_and_incomplete_torus_are_not_edge_blends() -> None:
    full = Torus(10, 2)
    bead = Cylinder(10, 20) + Torus(10, 2)
    incomplete = Torus(10, 2) & Pos(0, -12, -3) * Box(12, 24, 6)
    non_tangent = Cylinder(10, 20) + Pos(0, 0, 10) * Torus(9, 2)

    assert recognise_blends(full) == []
    assert recognise_blends(bead) == []
    assert recognise_blends(incomplete) == []
    assert recognise_blends(non_tangent) == []


def test_subdivided_torus_is_one_complete_circular_path() -> None:
    part = _internal_toroidal_bottom()
    torus_face = next(
        face
        for face in part.faces()
        if BRepAdaptor_Surface(face.wrapped).GetType() == GeomAbs_Torus
    )
    adaptor = BRepAdaptor_Surface(torus_face.wrapped)
    surface = BRep_Tool.Surface_s(torus_face.wrapped)
    seam = BRepBuilderAPI_MakeEdge(
        surface.UIso(1.0), adaptor.FirstVParameter(), adaptor.LastVParameter()
    ).Edge()
    splitter = BRepFeat_SplitShape(part.wrapped)
    splitter.Add(seam, torus_face.wrapped)
    splitter.Build()
    assert splitter.IsDone()
    split = type(part).cast(splitter.Shape())

    assert sum(face.geom_type == GeomType.TORUS for face in split.faces()) == 2
    assert recognise_blends(split) == recognise_blends(part)
    view = build_recognition_evidence(split)
    (feature,) = [item for item in view.features if view.family(item) == "blends"]
    assert len(view.defining_faces(feature)) == 2
    assert view.constituent_faces(feature) == view.defining_faces(feature)


def test_circular_blind_step_is_not_a_complete_blend_chain() -> None:
    product = _take_inventory(_circular_blind_step())
    blends = product.physical.candidate_set(FamilyId.BLENDS).candidates
    steps = product.physical.candidate_set(FamilyId.CIRCULAR_BLIND_STEPS).candidates

    assert blends == ()
    assert len(steps) == 1
    assert product.result.blends == ()
    assert product.reconciliation.for_family(FamilyId.BLENDS) == ()


def test_annular_boss_and_hole_decomposition_is_not_a_blend() -> None:
    product = _take_inventory(_annular_boss())

    assert product.result.blends == ()
    assert product.result.bosses
    assert product.result.holes


def test_parallel_wall_circular_slot_ends_are_not_edge_blends() -> None:
    part = _obround_passage()

    assert recognise_blends(part) == []
    assert _take_inventory(part).result.blends == ()


def test_oblique_chain_retains_canonical_free_axis_and_rigid_translation() -> None:
    rotated = Rot(20, 30, 40) * _external(0.2)
    shifted = Pos(13, -7, 5) * rotated
    before = recognise_blends(rotated)
    after = recognise_blends(shifted)

    assert len(before) == len(after) == 4
    for left, right in zip(before, after, strict=True):
        assert isinstance(left.path, StraightBlendPath)
        assert isinstance(right.path, StraightBlendPath)
        assert left.radius == right.radius == 0.2
        assert left.side == right.side == "convex"
        assert left.path.direction == pytest.approx(right.path.direction, abs=1e-12)
        assert math.hypot(*left.path.direction) == pytest.approx(1.0)
        assert right.path.at == pytest.approx(
            (left.path.at[0] + 13, left.path.at[1] - 7, left.path.at[2] + 5),
            abs=1e-3,
        )


def test_concave_chains_retain_side_radius_and_direction_under_rigid_motion() -> None:
    rotated = Rot(20, 30, 40) * _internal()
    shifted = Pos(13, -7, 5) * rotated
    before = recognise_blends(rotated)
    after = recognise_blends(shifted)

    assert len(before) == len(after) == 4
    for left, right in zip(before, after, strict=True):
        assert isinstance(left.path, StraightBlendPath)
        assert isinstance(right.path, StraightBlendPath)
        assert left.side == right.side == "concave"
        assert left.radius == right.radius == 2
        assert left.path.direction == pytest.approx(right.path.direction, abs=1e-12)
        assert right.path.at == pytest.approx(
            (left.path.at[0] + 13, left.path.at[1] - 7, left.path.at[2] + 5),
            abs=1e-3,
        )


def test_circular_paths_are_free_axis_and_translation_covariant() -> None:
    rotated = Rot(20, 30, 40) * _internal_toroidal_bottom()
    shifted = Pos(13, -7, 5) * rotated
    (before,) = recognise_blends(rotated)
    (after,) = recognise_blends(shifted)

    assert isinstance(before.path, CircularBlendPath)
    assert isinstance(after.path, CircularBlendPath)
    assert before.side == after.side == "concave"
    assert before.radius == after.radius == 1
    assert before.path.radius == after.path.radius == 4
    assert before.path.normal == pytest.approx(after.path.normal, abs=1e-12)
    assert after.path.center == pytest.approx(
        (before.path.center[0] + 13, before.path.center[1] - 7, before.path.center[2] + 5),
        abs=1e-3,
    )


@pytest.mark.parametrize(
    "angles", [(0, 0, 0), (90, 0, 0), (0, 90, 0), (17, 31, 43), (71, 19, -113)]
)
def test_circular_blend_raw_and_framed_paths_agree_under_rigid_motion(angles) -> None:
    part = Pos(13, -7, 5) * Rot(*angles) * _internal_toroidal_bottom()
    (standalone,) = recognise_blends(part)
    (raw,) = build_raw_recognition_result(part).blends
    framed = build_framed_recognition_result(part)
    assert isinstance(framed, FramedRecognitionResult)
    (local,) = framed.result.blends
    assert standalone == raw
    assert raw.side == local.side == "concave"
    assert raw.radius == local.radius == 1.0
    assert isinstance(raw.path, CircularBlendPath)
    assert isinstance(local.path, CircularBlendPath)
    assert raw.path.radius == local.path.radius == 4.0
    assert raw.path.center == pytest.approx(framed.frame.to_world(local.path.center), abs=2e-3)
    axes = (framed.frame.x, framed.frame.y, framed.frame.z)
    world_normal = tuple(
        sum(local.path.normal[j] * axes[j][i] for j in range(3)) for i in range(3)
    )
    # A circular plane normal is unoriented; public canonicalization may reverse its sign.
    dot = sum(a * b for a, b in zip(raw.path.normal, world_normal, strict=True))
    assert abs(dot) == pytest.approx(1.0, abs=2e-6)
    view = build_recognition_evidence(part)
    feature, = [item for item in view.features if view.family(item) == "blends"]
    assert view.record(feature) == raw
    assert len(view.defining_faces(feature)) == 1
    assert view.constituent_faces(feature) == view.defining_faces(feature)
    face = view.face(next(iter(view.defining_faces(feature))))
    assert BRepAdaptor_Surface(face.wrapped).GetType() == GeomAbs_Torus


def test_rotated_partial_circular_edge_path_remains_unsupported() -> None:
    half = _internal_toroidal_bottom() & Pos(15, 0, 0) * Box(30, 60, 20)
    assert recognise_blends(Rot(17, 31, 43) * half) == []


def test_mirror_preserves_circular_path_side_and_dimensions() -> None:
    base = recognise_blends(_internal_toroidal_bottom())
    mirrored = recognise_blends(_internal_toroidal_bottom().mirror(Plane.YZ))

    assert len(base) == len(mirrored) == 1
    assert base[0].side == mirrored[0].side == "concave"
    assert base[0].radius == mirrored[0].radius == 1
    assert isinstance(base[0].path, CircularBlendPath)
    assert isinstance(mirrored[0].path, CircularBlendPath)
    assert base[0].path.radius == mirrored[0].path.radius == 4


@pytest.mark.parametrize("factor", (0.05, 5.0, 100.0))
def test_uniform_scale_preserves_occurrences_and_scales_dimensions(factor: float) -> None:
    base = recognise_blends(_external(0.2))
    scaled = recognise_blends(_external(0.2).scale(factor))

    assert len(base) == len(scaled) == 4
    for left, right in zip(base, scaled, strict=True):
        assert isinstance(left.path, StraightBlendPath)
        assert isinstance(right.path, StraightBlendPath)
        assert right.side == left.side == "convex"
        assert right.path.direction == pytest.approx(left.path.direction, abs=1e-12)
        assert right.radius == pytest.approx(left.radius * factor)
        # Each public anchor is independently quantized to 0.001 model units.
        assert right.path.at == pytest.approx(
            tuple(value * factor for value in left.path.at),
            abs=max(6e-4, 6e-4 * factor),
        )


@pytest.mark.parametrize("factor", (0.05, 5.0, 100.0))
def test_uniform_scale_preserves_concave_occurrences(factor: float) -> None:
    base = recognise_blends(_internal())
    scaled = recognise_blends(_internal().scale(factor))

    assert len(base) == len(scaled) == 4
    for left, right in zip(base, scaled, strict=True):
        assert isinstance(left.path, StraightBlendPath)
        assert isinstance(right.path, StraightBlendPath)
        assert right.side == left.side == "concave"
        assert right.path.direction == pytest.approx(left.path.direction, abs=1e-12)
        assert right.radius == pytest.approx(left.radius * factor)


@pytest.mark.parametrize("factor", (0.05, 5.0, 100.0))
def test_uniform_scale_preserves_circular_blend_paths(factor: float) -> None:
    (base,) = recognise_blends(_internal_toroidal_bottom())
    (scaled,) = recognise_blends(_internal_toroidal_bottom().scale(factor))

    assert isinstance(base.path, CircularBlendPath)
    assert isinstance(scaled.path, CircularBlendPath)
    assert scaled.side == base.side == "concave"
    assert scaled.path.normal == pytest.approx(base.path.normal, abs=1e-12)
    assert scaled.radius == pytest.approx(base.radius * factor)
    assert scaled.path.radius == pytest.approx(base.path.radius * factor)
    assert scaled.path.center == pytest.approx(
        tuple(value * factor for value in base.path.center),
        abs=max(6e-4, 6e-4 * factor),
    )


def test_compound_keeps_equal_looking_chains_body_local() -> None:
    part = Compound(children=[Pos(-60, 0, 0) * _external(0.2), Pos(60, 0, 0) * _external(0.2)])
    view = build_recognition_evidence(part)
    blend_refs = [feature for feature in view.features if view.family(feature) == "blends"]

    assert len(view.result.blends) == len(blend_refs) == 8
    assert len({frozenset(view.defining_faces(feature)) for feature in blend_refs}) == 8


def test_compound_keeps_equal_looking_concave_chains_body_local() -> None:
    part = Compound(children=[Pos(-60, 0, 0) * _internal(), Pos(60, 0, 0) * _internal()])
    view = build_recognition_evidence(part)
    blend_refs = [feature for feature in view.features if view.family(feature) == "blends"]

    assert len(view.result.blends) == len(blend_refs) == 8
    assert all(view.record(feature).side == "concave" for feature in blend_refs)
    assert len({frozenset(view.defining_faces(feature)) for feature in blend_refs}) == 8


def test_compound_keeps_equal_toroidal_paths_body_local() -> None:
    part = Compound(
        children=[
            Pos(-60, 0, 0) * _internal_toroidal_bottom(),
            Pos(60, 0, 0) * _internal_toroidal_bottom(),
        ]
    )
    view = build_recognition_evidence(part)
    refs = [feature for feature in view.features if view.family(feature) == "blends"]

    assert len(view.result.blends) == len(refs) == 2
    assert len({frozenset(view.defining_faces(feature)) for feature in refs}) == 2


def test_step_round_trip_preserves_toroidal_blend_records(tmp_path) -> None:
    part = _internal_toroidal_bottom()
    path = tmp_path / "toroidal-blend.step"
    assert export_step(part, path)

    assert recognise_blends(import_step_geometry(path)) == recognise_blends(part)


def test_sharp_stock_and_full_cylinder_are_not_blends() -> None:
    assert recognise_blends(Box(40, 30, 20)) == []
    assert recognise_blends(Cylinder(10, 20)) == []


def test_face_traversal_order_does_not_change_records(monkeypatch) -> None:
    part = _external(0.2)
    baseline = _rounded_signature(part)
    assert len(baseline) == 4
    part_type = type(part)
    real_faces = part_type.faces

    def reversed_faces(self):
        faces = real_faces(self)
        return type(faces)(reversed(faces))

    monkeypatch.setattr(part_type, "faces", reversed_faces)
    assert _rounded_signature(part) == baseline


def test_toroidal_face_traversal_order_does_not_change_records(monkeypatch) -> None:
    part = _turned_toroidal()
    baseline = recognise_blends(part)
    assert len(baseline) == 4
    part_type = type(part)
    real_faces = part_type.faces

    def reversed_faces(self):
        faces = real_faces(self)
        return type(faces)(reversed(faces))

    monkeypatch.setattr(part_type, "faces", reversed_faces)
    assert recognise_blends(part) == baseline
