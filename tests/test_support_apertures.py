from __future__ import annotations

import pytest
from build123d import Box, Compound, Cylinder, Face, Pos, Rot, Wire, export_step, import_step

from quiddity import build_section_recess_document
from quiddity._adjacency import FaceGraph
from quiddity._effective_surfaces import EffectiveSurfaceIndex
from quiddity._support_apertures import proved_support_apertures
from quiddity._support_patches import covered_patch


def _channel(*, floor=False, walls=False, scale=1):
    s = scale
    part = (
        Box(50 * s, 50 * s, 12 * s)
        + Pos(0, -18.75 * s, 15 * s) * Box(50 * s, 12.5 * s, 18 * s)
        + Pos(0, 18.75 * s, 15 * s) * Box(50 * s, 12.5 * s, 18 * s)
    )
    if floor:
        part = part - Cylinder(2 * s, 12 * s) - Pos(0, 0, 4 * s) * Cylinder(6 * s, 4 * s)
    if walls:
        part -= Pos(0, 0, 15 * s) * Rot(90, 0, 0) * Cylinder(4 * s, 60 * s)
    return part


def _patches(scale=1, placement=None):
    if placement is None:
        placement = Rot()
    points = (
        ((-25, -12.5, 6), (25, -12.5, 6), (25, 12.5, 6), (-25, 12.5, 6)),
        ((-25, -12.5, 6), (25, -12.5, 6), (25, -12.5, 24), (-25, -12.5, 24)),
        ((-25, 12.5, 6), (25, 12.5, 6), (25, 12.5, 24), (-25, 12.5, 24)),
    )
    return tuple(
        placement
        * Face(Wire.make_polygon([tuple(v * scale for v in point) for point in loop], close=True))
        for loop in points
    )


def _prove(part, patches):
    graph = FaceGraph(part)
    surfaces = EffectiveSurfaceIndex(graph)
    outcomes = []
    for patch in patches:
        nodes = [
            node
            for node in graph.nodes
            if graph.is_planar(node) and covered_patch(graph.face(node), (patch,))
        ]
        assert len(nodes) == 1
        support = nodes[0]
        proofs = proved_support_apertures(graph, surfaces, support, patch)
        outcomes.append(
            (
                covered_patch(patch, (graph.face(support), *(p.disk for p in proofs))),
                proofs,
                support,
            )
        )
    return graph, outcomes


@pytest.mark.parametrize(
    "floor,walls", [(False, False), (True, False), (False, True), (True, True)]
)
@pytest.mark.parametrize("rotation", [Rot(), Rot(17, 31, 43), Rot(180, 0, 0)])
@pytest.mark.parametrize("scale", [0.1, 1, 10])
def test_complete_apertures_explain_only_the_original_support_deficit(
    floor, walls, rotation, scale
):
    placement = Pos(3, 7, 11) * rotation
    graph, outcomes = _prove(
        placement * _channel(floor=floor, walls=walls, scale=scale), _patches(scale, placement)
    )
    assert all(complete for complete, _, _ in outcomes)
    assert [len(proofs) for _, proofs, _ in outcomes] == [int(floor), int(walls), int(walls)]
    for _, proofs, support in outcomes:
        for proof in proofs:
            assert graph.common_valid_solid((support, proof.cylinder)) is not None
    cylinders = [p.cylinder for _, proofs, _ in outcomes for p in proofs]
    assert len(cylinders) == len(set(cylinders))  # Never join opposite wall segments.


@pytest.mark.parametrize(
    "kind", ["end_touch", "end_breakout", "oblique", "obstruction", "extra_gap"]
)
def test_unexplained_or_noninterior_support_is_not_repaired(kind):
    part = _channel(floor=True)
    if kind in {"end_touch", "end_breakout"}:
        x = 21 if kind == "end_touch" else 22
        part -= Pos(x, 0, 15) * Rot(90, 0, 0) * Cylinder(4, 60)
    elif kind == "oblique":
        part -= Pos(0, 0, 15) * Rot(80, 0, 0) * Cylinder(4, 60)
    elif kind == "obstruction":
        part -= Pos(0, 0, 15) * Rot(90, 0, 0) * Cylinder(4, 60)
        part += Pos(0, 18.75, 15) * Box(10, 1, 1)
    else:
        part -= Pos(24, 0, 6) * Box(4, 2, 4)
    _, outcomes = _prove(part, _patches())
    assert not all(complete for complete, _, _ in outcomes)
    assert not any(
        record.classification.feature_kind == "channel"
        for record in build_section_recess_document(part).occurrences
    )


def test_distinct_bodies_and_step_round_trip(tmp_path):
    part = Compound(
        [_channel(floor=True, walls=True), Pos(100, 0, 0) * _channel(floor=True, walls=True)]
    )
    path = tmp_path / "pierced-channels.step"
    export_step(part, path)
    for shape in (part, import_step(path)):
        graph, outcomes = _prove(shape, (*_patches(), *_patches(placement=Pos(100, 0, 0))))
        assert all(complete for complete, _, _ in outcomes)
        assert all(len(proofs) == 1 for _, proofs, _ in outcomes)
        owners = [
            graph.common_valid_solid((support, proofs[0].cylinder))
            for _, proofs, support in outcomes
        ]
        assert owners[0] == owners[1] == owners[2]
        assert owners[3] == owners[4] == owners[5]
        assert owners[0] != owners[3]
        document = build_section_recess_document(shape)
        assert len(document.occurrences) == 2
        assert len({record.body for record in document.occurrences}) == 2
        assert all(len(record.evidence.constituent_faces) == 6 for record in document.occurrences)


@pytest.mark.parametrize("x,accepted", [(20.9999, True), (21.0, False), (21.0001, False)])
def test_aperture_must_stay_strictly_inside_the_longitudinal_end(x, accepted):
    part = _channel() - Pos(x, 0, 15) * Rot(90, 0, 0) * Cylinder(4, 60)
    records = build_section_recess_document(part).occurrences
    assert any(record.classification.feature_kind == "channel" for record in records) == accepted


@pytest.mark.parametrize(
    "floor,walls", [(False, False), (True, False), (False, True), (True, True)]
)
@pytest.mark.parametrize("rotation", [Rot(), Rot(90, 0, 0), Rot(0, 90, 0), Rot(180, 0, 0)])
@pytest.mark.parametrize("asymmetric", [False, True])
def test_public_channel_retains_base_geometry_and_original_aperture_evidence(
    floor, walls, rotation, asymmetric
):
    plain = _channel()
    pierced = _channel(floor=floor, walls=walls)
    if asymmetric:
        extension = Pos(0, 18.75, 26) * Box(50, 12.5, 4)
        plain += extension
        pierced += extension
    placement = Pos(3, 7, 11) * rotation
    original = build_section_recess_document(placement * plain)
    actual = build_section_recess_document(placement * pierced)
    assert len(original.occurrences) == len(actual.occurrences) == 1
    assert not actual.refusals
    record = actual.occurrences[0]
    assert record.classification.feature_kind == "channel"
    assert record.geometry == original.occurrences[0].geometry
    assert len(record.evidence.defining_faces) == 2
    assert len(record.evidence.constituent_faces) == 3 + int(floor) + 2 * int(walls)
    graph = FaceGraph(placement * pierced)
    assert (
        graph.common_valid_solid(tuple(graph.nodes[i] for i in record.evidence.constituent_faces))
        is not None
    )
