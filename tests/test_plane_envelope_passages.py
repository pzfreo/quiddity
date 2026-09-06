"""Authored original-face proofs for ADR 0024, independent of datasets."""

import json
from dataclasses import replace

import pytest
from build123d import (
    Box,
    Compound,
    Face,
    Keep,
    Plane,
    Pos,
    RegularPolygon,
    Rot,
    Solid,
    Vector,
    Wire,
    export_step,
    extrude,
    import_step,
)

from quiddity import build_section_recess_document
from quiddity._adjacency import FaceGraph
from quiddity._plane_envelope_passages import plane_envelope_passage_proofs
from quiddity._section_recess_geometry import _plane_envelope_geometry
from quiddity._sections import LocalFrame


def roof_passage(sides=6, scale=1):
    stock = Box(40 * scale, 40 * scale, 40 * scale).split(
        Plane(origin=(0, 0, 20 * scale), z_dir=(0.2, 0, 1)), Keep.BOTTOM
    )
    return stock - Pos(0, 0, -25 * scale) * extrude(RegularPolygon(3 * scale, sides), 50 * scale)


@pytest.mark.parametrize("sides,volume", [(3, 466.614487559), (4, 718.2), (6, 932.579456065)])
@pytest.mark.parametrize("scale", [0.1, 1])
@pytest.mark.parametrize("rotation", [Rot(), Rot(180, 0, 0), Rot(17, 31, 43)])
def test_original_roof_wall_and_mouth_proof(sides, volume, scale, rotation):
    graph = FaceGraph(Pos(13.2, -7.4, 4.1) * rotation * roof_passage(sides, scale))
    (proof,) = plane_envelope_passage_proofs(graph)
    assert len(proof.walls) == sides
    assert proof.volume == pytest.approx(volume * scale**3, rel=1e-7)
    assert len(proof.roof_contexts) == 2
    assert set(proof.walls).isdisjoint(proof.roof_contexts)
    assert (
        graph.common_valid_solid((*proof.walls, proof.planar_context, *proof.roof_contexts))
        is proof.owner
    )


def test_material_bridge_refuses():
    part = roof_passage() + Box(1, 12, 1)
    assert plane_envelope_passage_proofs(FaceGraph(part)) == ()


@pytest.mark.parametrize(
    "sides,slope,ridge,rotation",
    [
        (3, 0.08, -0.6, Rot()),
        (4, 0.65, 0.7, Rot(17, 31, 43)),
        (6, -0.35, 1.2, Rot(180, 0, 0)),
        (6, 1.2, -1.1, Rot(17, 31, 43)),
    ],
)
def test_off_centre_ridges_and_different_slopes(sides, slope, ridge, rotation):
    stock = Box(40, 40, 40).split(Plane(origin=(ridge, 0, 20), z_dir=(slope, 0, 1)), Keep.BOTTOM)
    tool = Pos(0, 0, -25) * extrude(RegularPolygon(3, sides), 50)
    cut_stock = stock - tool
    expected_volume = stock.volume - cut_stock.volume
    part = Pos(13.2, -7.4, 4.1) * rotation * cut_stock
    (proof,) = plane_envelope_passage_proofs(FaceGraph(part))
    assert len(proof.walls) == sides
    assert proof.volume == pytest.approx(expected_volume, rel=1e-7)
    document = build_section_recess_document(part)
    (record,) = document.occurrences
    assert record.classification.feature_kind == "passage"
    cell = reconstruct_json(json.loads(json.dumps(record.geometry.to_dict())))
    assert cell.is_valid and len(cell.solids()) == 1
    assert cell.volume == pytest.approx(expected_volume, rel=1e-4)
    assert not document.refusals


@pytest.mark.parametrize("kind", ["valley", "step", "breakout"])
def test_unsupported_terminal_and_side_topologies_refuse(kind):
    cut = Pos(0, 0, -25) * extrude(RegularPolygon(3, 6), 60)
    if kind == "valley":
        section = Face(
            Wire.make_polygon(
                [
                    (-20, -20, -20),
                    (20, -20, -20),
                    (20, -20, 24),
                    (0, -20, 20),
                    (-20, -20, 24),
                ],
                close=True,
            )
        )
        part = Solid.extrude(section, (0, 40, 0)) - cut
    elif kind == "step":
        part = Box(40, 40, 40) - Pos(10, 0, 20) * Box(20, 40, 20) - cut
    else:
        part = roof_passage() - Pos(10, 0, 0) * Box(20, 1, 4)
    assert plane_envelope_passage_proofs(FaceGraph(part)) == ()


def test_equal_roof_passages_keep_independent_owners_and_face_evidence():
    part = Compound([roof_passage(), Pos(70, 0, 0) * roof_passage()])
    first, second = plane_envelope_passage_proofs(FaceGraph(part))
    assert first.owner is not second.owner
    assert set(first.walls).isdisjoint(second.walls)
    document = build_section_recess_document(part)
    assert len(document.occurrences) == 2
    assert {r.body for r in document.occurrences} == {0, 1}


def reconstruct_json(value):
    frame = value["frame"]
    origin, u, v, run = (Vector(*frame[name]) for name in ("origin", "u", "v", "run"))
    points = [p["point"] for p in value["profile"]["boundary"]]
    end_planes = []
    for index, name in enumerate(("low", "high")):
        surface = value["ends"][name]["surface"]
        terms = (
            surface["terms"]
            if surface["type"] == "plane_envelope"
            else [{"height": value["run_interval"][index], "gradient": surface["gradient"]}]
        )
        end_planes.extend((index, t["height"], t["gradient"]) for t in terms)
    heights = [h + g[0] * p[0] + g[1] * p[1] for _, h, g in end_planes for p in points]
    lo, hi = min(heights) - 1, max(heights) + 1
    face = Face(
        Wire.make_polygon([origin + u * p[0] + v * p[1] + run * lo for p in points], close=True)
    )
    cell = Solid.extrude(face, run * (hi - lo))
    for index, h, g in end_planes:
        normal = (u + run * g[0]).cross(v + run * g[1])
        if index == 0:
            normal = -normal
        cell = cell.split(Plane(origin=origin + run * h, z_dir=normal), Keep.BOTTOM)
    return cell


@pytest.mark.parametrize("rotation", [Rot(), Rot(180, 0, 0), Rot(17, 31, 43)])
def test_public_json_reconstructs_the_two_plane_removal(rotation):
    part = Pos(13.2, -7.4, 4.1) * rotation * roof_passage()
    document = build_section_recess_document(part)
    (record,) = document.occurrences
    assert record.classification.feature_kind == "passage"
    cell = reconstruct_json(json.loads(json.dumps(record.geometry.to_dict())))
    assert cell.is_valid and len(cell.solids()) == 1
    assert cell.volume == pytest.approx(932.579456065, rel=1e-4)
    assert not document.refusals


def test_step_round_trip_keeps_reconstructible_roof(tmp_path):
    part = Pos(13.2, -7.4, 4.1) * Rot(17, 31, 43) * roof_passage()
    path = tmp_path / "two-plane-passage.step"
    before = build_section_recess_document(part)
    export_step(part, path)
    after = build_section_recess_document(import_step(path))
    assert len(after.occurrences) == 1
    assert after.occurrences[0].geometry == before.occurrences[0].geometry
    assert after.occurrences[0].classification == before.occurrences[0].classification


def test_projection_failure_preserves_an_independent_pocket(monkeypatch):
    import quiddity._section_recess_geometry as implementation

    pocket = Pos(80, 0, 0) * (Box(40, 40, 40) - Pos(0, 0, 15) * Box(8, 8, 20))
    part = Compound([roof_passage(), pocket])
    before = build_section_recess_document(part)
    assert len(before.occurrences) == 2

    def fail(_proof):
        raise ValueError("authored projection refusal")

    monkeypatch.setattr(implementation, "_plane_envelope_geometry", fail)
    after = build_section_recess_document(part)
    (survivor,) = after.occurrences
    assert survivor.classification.feature_kind == "pocket"
    assert survivor.geometry in [r.geometry for r in before.occurrences]


def test_large_run_coordinate_refuses_excessive_serialized_frame_error():
    (proof,) = plane_envelope_passage_proofs(FaceGraph(roof_passage()))
    frame = LocalFrame.canonical((1, 2, 3), (0, 0, 0))
    moved = replace(
        proof,
        frame=frame,
        run_interval=tuple(h + 1e6 for h in proof.run_interval),
        terms=tuple((h + 1e6, g) for h, g in proof.terms),
    )
    with pytest.raises(ValueError, match="whole-occurrence displacement"):
        _plane_envelope_geometry(moved)
