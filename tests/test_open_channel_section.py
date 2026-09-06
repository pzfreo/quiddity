"""Independent physical support and refusal checks for the two-ended channel proof."""

import pytest
from build123d import Box, Compound, Cylinder, Pos, Rot, Vertex, export_step, import_step

import quiddity._open_channel_section as proof_module
from quiddity import build_section_recess_document
from quiddity._adjacency import FaceGraph
from quiddity._effective_surfaces import EffectiveSurfaceIndex
from tools._legacy_recognition import recognise_channels


def channel(scale=1):
    return Box(80 * scale, 60 * scale, 30 * scale) - Pos(0, 0, 7.5 * scale) * Box(
        80 * scale, 20 * scale, 15 * scale
    )


def proof(part):
    # Start from the original candidate to challenge geometry validation independently of
    # whether the old detector also rejects the modified anatomy.
    (record,) = recognise_channels(channel())
    graph = FaceGraph(part)
    defining = frozenset(
        node
        for node in graph.nodes
        if graph.is_planar(node)
        and abs(graph.normal(node)[1]) > 0.999
        and all(abs(abs(vertex.Y) - 10) < 1e-6 for vertex in graph.face(node).vertices())
    )
    return proof_module.prove_open_channel(
        graph, defining, frozenset(graph.nodes), record, surfaces=EffectiveSurfaceIndex(graph)
    )


@pytest.mark.parametrize("scale", [0.1, 1, 10])
@pytest.mark.parametrize("rotation", [Rot(), Rot(90, 0, 0), Rot(0, 90, 0), Rot(180, 0, 0)])
def test_channel_reconstructs_only_physical_support(scale, rotation):
    part = Pos(123, -57, 91) * rotation * channel(scale)
    document = build_section_recess_document(part)
    (record,) = document.occurrences
    assert document.refusals == ()
    assert record.classification.feature_kind == "channel"
    geometry = record.geometry
    assert geometry.run_interval[1] - geometry.run_interval[0] == pytest.approx(80 * scale)
    frame = geometry.frame
    mid = sum(geometry.run_interval) / 2
    points = [
        tuple(
            frame.origin[i]
            + frame.run[i] * mid
            + frame.u[i] * vertex.point[0]
            + frame.v[i] * vertex.point[1]
            for i in range(3)
        )
        for vertex in geometry.profile.boundary
    ]
    faces = list(part.faces())
    for start, end in zip(points, points[1:], strict=False):
        midpoint = Vertex(*((a + b) / 2 for a, b in zip(start, end, strict=True)))
        assert (
            min(faces[index].distance_to(midpoint) for index in record.evidence.constituent_faces)
            < 0.002
        )
    gap = Vertex(*((a + b) / 2 for a, b in zip(points[0], points[-1], strict=True)))
    assert min(face.distance_to(gap) for face in faces) > scale


def test_channel_step_roundtrip(tmp_path):
    path = tmp_path / "channel.step"
    export_step(channel(), path)
    actual = build_section_recess_document(import_step(path))
    expected = build_section_recess_document(channel())
    assert [r.geometry for r in actual.occurrences] == [r.geometry for r in expected.occurrences]


@pytest.mark.parametrize(
    "part",
    [
        channel() - Pos(39, 0, 0) * Cylinder(2, 60),  # floor hole breaks the run end
        channel() + Pos(0, 0, 7) * Box(3, 24, 3),  # suspended crossbar
        channel() + Pos(39, 0, 7.5) * Box(2, 20, 15),  # capped run
        Compound([channel(), Pos(100, 0, 0) * channel()]),  # no cross-body proof
    ],
)
def test_incomplete_support_material_and_mixed_ownership_refuse(part):
    assert proof(channel()) is not None
    assert proof(part) is None


@pytest.mark.parametrize("stage", [0, 1, 2, 3])
def test_each_void_probe_can_independently_refuse(monkeypatch, stage):
    calls = []

    def material(*args):
        calls.append(args)
        return 1.0 if len(calls) == stage + 1 else 0.0

    monkeypatch.setattr(proof_module, "_material_fraction", material)
    assert proof(channel()) is None
    assert len(calls) == stage + 1


def test_boolean_failure_refuses_geometry(monkeypatch):
    def failed(*_args):
        raise RuntimeError("kernel operation failed")

    monkeypatch.setattr(proof_module, "_material_fraction", failed)
    assert proof(channel()) is None
