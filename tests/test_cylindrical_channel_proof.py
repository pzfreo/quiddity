"""Original-face checks for the authored bore-ended three-support channel."""

import math

import pytest
from build123d import Align, Box, Cylinder, Plane, Pos, Rot, Solid, Vector, export_step, import_step

import quiddity._cylindrical_channels as proof_module
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._cylindrical_channels import prove_cylindrical_channel
from quiddity._effective_surfaces import EffectiveSurfaceIndex
from quiddity.result import _take_inventory


def channel(scale=1, radius=4):
    part = Box(
        13.55 * scale,
        11 * scale,
        80 * scale,
        align=(Align.MIN, Align.CENTER, Align.MIN),
    )
    part -= Pos(12.61 * scale, -scale, 0) * Box(
        0.94 * scale,
        2 * scale,
        62.13 * scale,
        align=(Align.MIN, Align.MIN, Align.MIN),
    )
    part -= (
        Pos(0, 0, 66 * scale)
        * Rot(0, 90, 0)
        * Cylinder(
            radius * scale,
            13.55 * scale,
            align=(Align.CENTER, Align.CENTER, Align.MIN),
        )
    )
    return part


def prove(part):
    return prove_product(_take_inventory(part))


def prove_product(product):
    (candidate,) = product.accepted.candidate_set(FamilyId.POCKETS).candidates
    record = candidate.record
    return prove_cylindrical_channel(
        product.context.graph,
        product.context.surfaces,
        product.evidence.defining_of(record),
        product.evidence.constituent_of(record),
        run_axis=record.long_axis,
        width_axis=record.width_axis,
        open_sign=record.open_sign,
    )


@pytest.mark.parametrize("scale", [0.1, 1, 10])
@pytest.mark.parametrize("rotation", [Rot(), Rot(180, 0, 0), Rot(0, 90, 0)])
def test_native_three_supports_end_on_observed_bore(scale, rotation):
    proof = prove(Pos(123, -57, 91) * rotation * channel(scale))
    assert proof is not None
    assert proof.volume == pytest.approx(116.63908461722733 * scale**3, rel=1e-7)
    assert proof.run_interval[1] - proof.run_interval[0] == pytest.approx(62 * scale)
    assert len(proof.supports) == 3
    assert proof.cylinder not in proof.supports
    assert proof.planar_context not in proof.supports


def test_small_bore_leaving_original_planar_cap_is_not_a_curved_channel():
    # The wall tips no longer reach the cylinder: small original cap remnants survive.
    assert prove(channel(radius=3.99)) is None


def test_original_step_faces_prove_same_curved_channel(tmp_path):
    path = tmp_path / "bore-ended-channel.step"
    export_step(channel(), path)
    actual = prove(import_step(path))
    assert actual is not None
    assert actual.volume == pytest.approx(116.63908461722733, rel=1e-7)
    assert actual.run_interval == pytest.approx((0, 62))


@pytest.mark.parametrize("stage", range(4))
def test_material_in_cell_or_any_opening_independently_refuses(monkeypatch, stage):
    # Discovery now invokes this proof too. Prepare its source evidence before
    # injecting a one-shot failure into the single proof invocation under test.
    product = _take_inventory(channel())
    calls = []

    def material(*args):
        calls.append(args)
        return 1.0 if len(calls) == stage + 1 else 0.0

    monkeypatch.setattr(proof_module, "material_fraction", material)
    assert prove_product(product) is None
    assert len(calls) == stage + 1


@pytest.mark.parametrize("radius_delta", [-0.0001, 0, 0.0001])
def test_oblique_bore_requires_strict_domain_at_every_corner(radius_delta):
    part = Box(13.55, 11, 80, align=(Align.MIN, Align.CENTER, Align.MIN))
    part -= Pos(12.61, -1, 0) * Box(0.94, 2, 66, align=(Align.MIN, Align.MIN, Align.MIN))
    axis = Vector(1, 1, 0).normalized()
    part -= Solid.make_cylinder(
        1.47 / math.sqrt(2) + radius_delta,
        100,
        Plane(origin=Vector(13.08, 0, 66) - axis * 50, z_dir=axis),
    )
    graph = FaceGraph(part)
    walls = frozenset(
        node
        for node in graph.nodes
        if graph.is_planar(node)
        and abs(graph.normal(node)[1]) > 0.999
        and abs(abs(graph.face(node).center().Y) - 1) < 1e-6
    )
    floors = frozenset(
        node
        for node in graph.nodes
        if graph.is_planar(node)
        and graph.normal(node)[0] > 0.999
        and abs(graph.face(node).center().X - 12.61) < 1e-6
    )
    assert len(walls) == 2 and len(floors) == 1
    proof = prove_cylindrical_channel(
        graph,
        EffectiveSurfaceIndex(graph),
        walls,
        walls | floors,
        run_axis="z",
        width_axis="y",
        open_sign=1,
    )
    assert (proof is not None) == (radius_delta > 0)
