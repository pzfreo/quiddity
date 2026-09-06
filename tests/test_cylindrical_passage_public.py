"""JSON-only reconstruction and aggregate publication of bore-ended passages."""

import json

import pytest
from build123d import Compound, Pos, Rot, Vertex

import quiddity._section_recess_geometry as geometry_module
from quiddity import build_raw_recognition_result, build_section_recess_document
from tests.test_cylindrical_channel_public import reconstruct
from tests.test_cylindrical_passage_proof import passage


@pytest.mark.parametrize("sides,volume", [(3, 141.127130), (4, 217.712177), (6, 283.374482)])
@pytest.mark.parametrize("scale", [0.1, 1])
@pytest.mark.parametrize("rotation", [Rot(), Rot(17, 31, 43)])
def test_public_json_reconstructs_each_observed_passage(sides, volume, scale, rotation):
    part = Pos(123.4567, -57.1234, 91.2345) * rotation * passage(sides, scale)
    document = build_section_recess_document(part)
    assert document.refusals == ()
    assert len(document.occurrences) == 2
    faces = list(part.faces())
    for record in document.occurrences:
        assert record.classification.feature_kind == "passage"
        assert len(record.evidence.defining_faces) == sides
        assert record.evidence.defining_faces == record.evidence.constituent_faces
        value = json.loads(json.dumps(record.to_dict()))["geometry"]
        assert value["profile"]["closure"] == "closed"
        assert all(end["condition"] == "open" for end in value["ends"].values())
        region = reconstruct(value)
        assert region.is_valid and len(region.solids()) == 1
        assert abs(region.volume - volume * scale**3) <= region.area * 0.002
        frame = value["frame"]
        points = [v["point"] for v in value["profile"]["boundary"]]
        middle = sum(value["run_interval"]) / 2
        for start, stop in zip(points, points[1:] + points[:1], strict=True):
            u, v = ((a + b) / 2 for a, b in zip(start, stop, strict=True))
            sample = Vertex(
                *(
                    frame["origin"][i]
                    + u * frame["u"][i]
                    + v * frame["v"][i]
                    + middle * frame["run"][i]
                    for i in range(3)
                )
            )
            assert (
                min(faces[n].distance_to(sample) for n in record.evidence.constituent_faces)
                <= 0.002
            )


def test_equal_public_passages_do_not_join_across_bodies_or_cross_bore():
    part = passage(6)
    result = build_raw_recognition_result(Compound([part, Pos(70, 0, 0) * part]))
    assert len(result.section_recesses) == 4
    assert {r.body for r in result.section_recesses} == {0, 1}
    assert len({n for r in result.section_recesses for n in r.evidence.constituent_faces}) == 24
    assert result.holes


def test_projection_refusal_does_not_abort_independent_bore(monkeypatch):
    def decline(*args, **kwargs):
        raise ValueError("injected bounded projection refusal")

    monkeypatch.setattr(geometry_module, "_cylindrical_geometry", decline)
    result = build_raw_recognition_result(passage(6))
    assert result.section_recesses == ()
    assert result.holes
