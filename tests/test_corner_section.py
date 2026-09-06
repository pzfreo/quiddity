# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Corner-notch projection must recover physical walls, not close a bounding rectangle."""

from itertools import combinations

import pytest
from build123d import Box, Compound, Cylinder, Pos, Rot, Vector, Vertex, export_step, import_step

from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._corner_section import prove_corner_section
from quiddity.result import _take_inventory


def corner(scale=1):
    return Box(60 * scale, 40 * scale, 12 * scale) - Pos(25 * scale, 15 * scale, 4 * scale) * Box(
        20 * scale, 20 * scale, 8 * scale
    )


def assert_truthful_corner(part):
    product = _take_inventory(part)
    (source,) = [
        candidate
        for candidate in product.accepted.candidate_set(FamilyId.POCKETS).candidates
        if candidate.record.edge_anchored
    ]
    defining = product.evidence.defining_of(source)
    matches = [
        record
        for record in product.result.section_recesses
        if set(record.evidence.defining_faces) == {node.index for node in defining}
    ]
    (record,) = matches
    assert record.classification.feature_kind == "edge_open_recess"
    assert record.classification.section_shape == "polygonal"
    assert len(record.geometry.profile.boundary) == 3
    assert len(record.evidence.constituent_faces) == 3
    frame = record.geometry.frame
    mid = sum(record.geometry.run_interval) / 2
    chain = [
        tuple(
            frame.origin[i]
            + frame.run[i] * mid
            + frame.u[i] * vertex.point[0]
            + frame.v[i] * vertex.point[1]
            for i in range(3)
        )
        for vertex in record.geometry.profile.boundary
    ]
    for start, end in zip(chain, chain[1:], strict=False):
        midpoint = Vertex(*((a + b) / 2 for a, b in zip(start, end, strict=True)))
        assert (
            min(product.context.graph.face(node).distance_to(midpoint) for node in defining) < 0.002
        )
    # Closing the chain would create a fictitious diagonal wall through empty space.
    gap_middle = Vertex(*((a + b) / 2 for a, b in zip(chain[0], chain[-1], strict=True)))
    assert min(product.context.graph.face(node).distance_to(gap_middle) for node in defining) > 0.02
    ends = record.geometry.ends
    assert ends.low.condition == ("capped" if source.record.open_sign == 1 else "open")
    assert ends.high.condition == ("open" if source.record.open_sign == 1 else "capped")


@pytest.mark.parametrize("scale", [0.1, 1, 10])
@pytest.mark.parametrize("rotation", [Rot(), Rot(90, 0, 0), Rot(0, 90, 0), Rot(180, 0, 0)])
def test_source_corner_chain_is_covariant_and_scale_independent(scale, rotation):
    assert_truthful_corner(Pos(123, -57, 91) * rotation * corner(scale))


def test_corner_survives_step_round_trip(tmp_path):
    path = tmp_path / "corner.step"
    export_step(corner(), path)
    assert_truthful_corner(import_step(path))


def test_equal_corners_on_separate_bodies_keep_distinct_ownership():
    product = _take_inventory(Compound([corner(), Pos(100, 0, 0) * corner()]))
    records = product.result.section_recesses
    assert len(records) == 2
    assert {record.body for record in records} == {0, 1}
    assert set(records[0].evidence.defining_faces).isdisjoint(records[1].evidence.defining_faces)


def test_box_corners_are_not_recesses():
    graph = FaceGraph(Box(10, 20, 30))
    for nodes in combinations(graph.nodes, 3):
        for axis in "xyz":
            assert prove_corner_section(graph, frozenset(nodes), axis) is None


def test_hole_in_corner_floor_is_not_replaced_by_a_full_rectangle():
    part = corner() - Pos(22, 12, -3) * Cylinder(1, 20)
    graph = FaceGraph(part)
    # The two notch walls and their perforated floor still meet at the inner trihedral corner.
    nodes = frozenset(
        node
        for node in graph.nodes
        if any(tuple(vertex) == pytest.approx((15, 5, 0)) for vertex in graph.face(node).vertices())
    )
    assert len(nodes) == 3
    assert prove_corner_section(graph, nodes, "z") is None


def test_non_owned_or_incomplete_face_sets_are_refused():
    graph = FaceGraph(Compound([corner(), Pos(100, 0, 0) * corner()]))
    assert prove_corner_section(graph, frozenset(), "z") is None
    assert prove_corner_section(graph, frozenset(graph.nodes[:3]), "bad") is None
    mixed = frozenset((graph.nodes[0], graph.nodes[1], graph.nodes[-1]))
    assert prove_corner_section(graph, mixed, "z") is None


def test_a_second_cap_is_not_called_an_open_run_end():
    part = corner() + Pos(0, 0, 7) * Box(60, 40, 2)
    graph = FaceGraph(part)
    nodes = frozenset(
        node
        for node in graph.nodes
        if any(tuple(vertex) == pytest.approx((15, 5, 0)) for vertex in graph.face(node).vertices())
    )
    assert len(nodes) == 3
    assert prove_corner_section(graph, nodes, "z") is None


def test_remaining_step_summaries_are_laterally_open_not_closed_pockets():
    from tests.golden.plates_pads_levels_and_slanted_steps.fixture import build_fixture

    part = build_fixture()
    product = _take_inventory(part)
    summaries = product._legacy_result.pockets
    assert len(summaries) == 2
    assert not any(record.edge_anchored for record in summaries)
    assert len(product.result.section_recesses) == 2
    assert all(
        item.classification.feature_kind == "channel"
        and item.geometry.ends.low.condition == item.geometry.ends.high.condition == "open"
        for item in product.result.section_recesses
    )
    # Under the overhang and between the wall and step: void persists beyond both lateral
    # ends of the opposed-wall overlap. No third/fourth closing walls exist at those ends.
    for x, z, lateral in ((37, 8, 10), (-25, 8, 26)):
        assert not part.is_inside(Vector(x, 0, z))
        assert not part.is_inside(Vector(x, -lateral, z))
        assert not part.is_inside(Vector(x, lateral, z))
    assert part.is_inside(Vector(37, 0, 14))  # pad above the overhang region
    assert part.is_inside(Vector(37, 0, 2))  # base below
    assert part.is_inside(Vector(-50, 0, 8))  # tall wall beside the second region
    assert part.is_inside(Vector(0, 0, 8))  # lower step on its other side


@pytest.mark.parametrize("xy", [(20, 10), (27, 17)])
@pytest.mark.parametrize("post_z", [8, 10])
@pytest.mark.parametrize("rotation", [Rot(), Rot(90, 0, 0), Rot(0, 90, 0), Rot(180, 0, 0)])
def test_suspended_material_in_run_or_mouth_refuses_corner_projection(xy, post_z, rotation):
    # Both positions lie over the floor. The second is outside the triangle that would result
    # from wrongly closing the open L chain. z=10 obstructs only the mouth, not the run interior.
    part = corner() + Pos(-20, -10, 9) * Box(5, 5, 6)
    part += Pos(0, 0, 13) * Box(60, 40, 2)
    part += Pos(*xy, post_z) * Box(3, 3, 8)
    assert len(part.solids()) == 1
    assert not part.is_inside(Vector(*xy, 2))
    assert part.is_inside(Vector(*xy, 5)) == (post_z == 8)
    assert part.is_inside(Vector(*xy, 6.1))
    product = _take_inventory(Pos(123, -57, 91) * rotation * part)
    # Discovery is unchanged: the accepted legacy summary must not become an unsupported
    # constant-section JSON occurrence merely because its three defining faces still exist.
    assert any(record.edge_anchored for record in product._legacy_result.pockets)
    assert not any(
        record.classification.feature_kind == "edge_open_recess"
        for record in product.result.section_recesses
    )


def test_material_probe_failure_refuses_projection_without_dropping_legacy_record(monkeypatch):
    import quiddity._corner_section as adapter

    def failure(*args):
        raise RuntimeError("authored boolean failure")

    monkeypatch.setattr(adapter, "_material_fraction", failure)
    product = _take_inventory(corner())
    assert any(record.edge_anchored for record in product._legacy_result.pockets)
    assert product.result.section_recesses == ()


def test_material_probe_uses_only_the_owning_body():
    part = Compound([corner(), Pos(20, 10, 3) * Box(2, 2, 2)])
    product = _take_inventory(part)
    (record,) = product.result.section_recesses
    assert record.body == 0
    assert len(record.geometry.profile.boundary) == 3
