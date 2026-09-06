"""Authored original-source tests for bore-ended polygonal passages."""

import pytest
from build123d import (
    Box,
    Compound,
    Cylinder,
    Pos,
    RegularPolygon,
    Rot,
    export_step,
    extrude,
    import_step,
)

import quiddity._cylindrical_passages as proof_module
from quiddity._adjacency import FaceGraph
from quiddity._cylindrical_passages import cylindrical_passage_proofs
from quiddity._effective_surfaces import EffectiveSurfaceIndex


def passage(sides, scale=1, *, bore=True):
    part = Box(40 * scale, 40 * scale, 40 * scale)
    if bore:
        part -= Rot(90, 0, 0) * Cylinder(8 * scale, 50 * scale)
    part -= (
        Rot(0, 90, 0)
        * Pos(0, 0, -25 * scale)
        * extrude(RegularPolygon(3 * scale, sides), 50 * scale)
    )
    return part


def proofs(part):
    graph = FaceGraph(part)
    return cylindrical_passage_proofs(graph, EffectiveSurfaceIndex(graph))


@pytest.mark.parametrize("sides,volume", [(3, 141.127130), (4, 217.712177), (6, 283.374482)])
@pytest.mark.parametrize("scale", [0.1, 1])
@pytest.mark.parametrize("rotation", [Rot(), Rot(17, 31, 43)])
def test_two_original_wall_rings_remain_distinct_across_one_bore(sides, volume, scale, rotation):
    result = proofs(Pos(13.2, -7.4, 4.1) * rotation * passage(sides, scale))
    assert len(result) == 2
    assert {proof.cylindrical_end for proof in result} == {0, 1}
    assert result[0].cylinder == result[1].cylinder
    assert not set(result[0].walls) & set(result[1].walls)
    for proof in result:
        assert len(proof.walls) == sides
        assert proof.volume == pytest.approx(volume * scale**3, rel=1e-7)
        assert proof.run_interval[0] < proof.run_interval[1]
        assert proof.cylinder not in proof.walls
        assert proof.planar_context not in proof.walls


@pytest.mark.parametrize("sides", [3, 4, 6])
def test_planar_through_cut_does_not_invent_a_cylindrical_end(sides):
    assert proofs(passage(sides, bore=False)) == ()


def test_step_round_trip_retains_two_original_hexagonal_regions(tmp_path):
    path = tmp_path / "bore-ended-hexagon.step"
    export_step(passage(6), path)
    result = proofs(import_step(path))
    assert len(result) == 2
    assert [p.volume for p in result] == pytest.approx([283.374482] * 2, rel=1e-7)


def test_equal_regions_in_two_bodies_keep_distinct_owners_and_supports():
    part = passage(6)
    result = proofs(Compound([part, Pos(70, 0, 0) * part]))
    assert len(result) == 4
    assert len({p.owner for p in result}) == 2
    assert len({n for p in result for n in p.walls}) == 24
    for p in result:
        assert len([other for other in result if other.owner == p.owner]) == 2


def test_blind_polygon_stopping_before_bore_is_not_a_passage():
    part = Box(40, 40, 40) - Rot(90, 0, 0) * Cylinder(8, 50)
    part -= Rot(0, 90, 0) * Pos(0, 0, -25) * extrude(RegularPolygon(3, 6), 13)
    assert proofs(part) == ()


def test_physical_bridge_refuses_obstructed_region_but_preserves_opposite():
    part = passage(6) + Pos(-14, 0, 0) * Box(1, 12, 12)
    (result,) = proofs(part)
    assert result.run_interval == pytest.approx((8, 20))


def test_partial_bore_does_not_replace_surviving_polygonal_wall_geometry():
    part = Box(40, 40, 40) - Rot(90, 0, 0) * Cylinder(2, 50)
    part -= Rot(0, 90, 0) * Pos(0, 0, -25) * extrude(RegularPolygon(3, 6), 50)
    assert proofs(part) == ()


def test_unequal_opposite_regions_keep_independent_planar_mouths():
    part = passage(6) - Pos(-19, 0, 0) * Box(4, 40, 40)
    result = sorted(proofs(part), key=lambda p: p.run_interval)
    assert len(result) == 2
    assert result[0].run_interval == pytest.approx((-17, -8))
    assert result[1].run_interval == pytest.approx((8, 20))


@pytest.mark.parametrize("radius", [2.9999, 3.0, 3.0001])
def test_source_branch_requires_strict_domain_at_extreme_polygon_vertices(radius):
    part = Box(40, 40, 40) - Rot(90, 0, 0) * Cylinder(radius, 50)
    part -= Rot(0, 90, 0) * Pos(0, 0, -25) * extrude(RegularPolygon(3, 6), 50)
    assert len(proofs(part)) == (2 if radius > 3 else 0)


def test_disconnected_wall_cycles_sharing_context_remain_separate():
    part = Box(40, 40, 40) - Rot(90, 0, 0) * Cylinder(8, 50)
    tool = Rot(0, 90, 0) * Pos(0, 0, -25) * extrude(RegularPolygon(1, 6), 50)
    part = part - Pos(0, -5, 0) * tool - Pos(0, 5, 0) * tool
    result = proofs(part)
    assert len(result) == 4
    assert len({p.owner for p in result}) == 1
    assert len({p.cylinder for p in result}) == 1
    assert len({p.planar_context for p in result}) == 2
    assert len({node for p in result for node in p.walls}) == 24
    assert all(len(p.walls) == 6 for p in result)


@pytest.mark.parametrize("stage", [0, 1, 2])
def test_each_empty_volume_gate_independently_refuses(monkeypatch, stage):
    graph = FaceGraph(passage(6))
    surfaces = EffectiveSurfaceIndex(graph)
    baseline = cylindrical_passage_proofs(graph, surfaces)[0]
    fact = surfaces.fact(baseline.cylinder)
    calls = []

    def material(*args):
        calls.append(args)
        return 1.0 if len(calls) == stage + 1 else 0.0

    monkeypatch.setattr(proof_module, "material_fraction", material)
    assert (
        proof_module._cell_proof(
            graph,
            baseline.planar_context,
            baseline.cylinder,
            fact,
            baseline.walls,
            proof_module.LocalFrame.canonical(baseline.frame.run, (0, 0, 0)),
        )
        is None
    )
    assert len(calls) == stage + 1


def test_native_surface_failure_is_local_to_its_candidate(monkeypatch):
    part = passage(6)
    graph = FaceGraph(Compound([part, Pos(70, 0, 0) * part]))
    surfaces = EffectiveSurfaceIndex(graph)
    baseline = cylindrical_passage_proofs(graph, surfaces)
    assert len(baseline) == 4
    refused = baseline[0].cylinder
    original = surfaces.fact

    def fact(node):
        if node == refused:
            raise RuntimeError("injected native surface refusal")
        return original(node)

    monkeypatch.setattr(surfaces, "fact", fact)
    result = cylindrical_passage_proofs(graph, surfaces)
    assert len(result) == 2
    assert all(p.cylinder != refused for p in result)
