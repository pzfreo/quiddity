"""Consumer reconstruction of the bore-ended channel using public JSON alone."""

import json
import math

import pytest
from build123d import Compound, Face, Plane, Pos, Rot, ShapeList, Solid, Vertex, Wire

import quiddity.result as result_module
from quiddity import build_section_recess_document
from quiddity._candidates import FamilyId
from quiddity.result import _matching_recesses, _take_inventory
from tests.test_cylindrical_channel_proof import channel


def reconstruct(value):
    frame = value["frame"]
    ends = value["ends"]
    end = next(
        i for i, name in enumerate(("low", "high")) if ends[name]["surface"]["type"] == "cylinder"
    )
    cylinder = ends[("low", "high")[end]]["surface"]
    flat = value["run_interval"][1 - end]
    points = [v["point"] for v in value["profile"]["boundary"]]

    def world(u, v, s):
        return tuple(
            frame["origin"][i] + u * frame["u"][i] + v * frame["v"][i] + s * frame["run"][i]
            for i in range(3)
        )

    cx, cy, cz = cylinder["axis_point"]
    a, b = cylinder["axis_direction"]
    norm = math.hypot(a, b)
    a, b = a / norm, b / norm
    base = Face(Wire.make_polygon([world(u, v, flat) for u, v in points], close=True))
    half = Solid.extrude(base, tuple((cz - flat) * x for x in frame["run"]))
    axial = [a * (u - cx) + b * (v - cy) for u, v in points]
    lo, hi = min(axial) - cylinder["radius"], max(axial) + cylinder["radius"]
    bore = Solid.make_cylinder(
        cylinder["radius"],
        hi - lo,
        Plane(
            origin=world(cx + a * lo, cy + b * lo, cz),
            z_dir=tuple(a * frame["u"][i] + b * frame["v"][i] for i in range(3)),
        ),
    )
    cut = half.cut(bore)
    return Compound(cut) if isinstance(cut, ShapeList) else cut


@pytest.mark.parametrize("scale", [0.1, 1, 10])
@pytest.mark.parametrize("rotation", [Rot(), Rot(180, 0, 0), Rot(0, 90, 0), Rot(90, 0, 0)])
def test_json_channel_preserves_curved_end_and_only_three_physical_supports(scale, rotation):
    part = Pos(123.4567, -57.1234, 91.2345) * rotation * channel(scale)
    document = build_section_recess_document(part)
    assert document.refusals == ()
    (record,) = document.occurrences
    assert record.classification.feature_kind == "channel"
    assert len(record.evidence.defining_faces) == 2
    assert len(record.evidence.constituent_faces) == 3
    geometry = json.loads(json.dumps(record.to_dict()))["geometry"]
    assert geometry["profile"]["closure"] == "open"
    assert all(end["condition"] == "open" for end in geometry["ends"].values())
    actual = reconstruct(geometry)
    assert actual is not None and actual.is_valid and len(actual.solids()) == 1
    # Independent authored volume; the tolerance is the existing publication
    # displacement budget times boundary area, not a new relaxed geometry bound.
    expected = 116.63908461722733 * scale**3
    assert abs(actual.volume - expected) <= actual.area * 0.002
    frame = geometry["frame"]
    points = [v["point"] for v in geometry["profile"]["boundary"]]
    mid = sum(geometry["run_interval"]) / 2
    faces = list(part.faces())
    for start, stop in zip(points, points[1:], strict=False):
        u, v = ((a + b) / 2 for a, b in zip(start, stop, strict=True))
        sample = Vertex(
            *(
                frame["origin"][i] + u * frame["u"][i] + v * frame["v"][i] + mid * frame["run"][i]
                for i in range(3)
            )
        )
        assert (
            min(faces[index].distance_to(sample) for index in record.evidence.constituent_faces)
            <= 0.002
        )


def test_equal_channels_on_separate_solids_keep_distinct_support_ownership():
    part = Compound(children=[channel(), Pos(100, 0, 0) * channel()])
    document = build_section_recess_document(part)
    assert document.refusals == ()
    assert len(document.occurrences) == 2
    assert {record.body for record in document.occurrences} == {0, 1}
    first, second = document.occurrences
    assert set(first.evidence.constituent_faces).isdisjoint(second.evidence.constituent_faces)
    for record in document.occurrences:
        geometry = json.loads(json.dumps(record.to_dict()))["geometry"]
        assert reconstruct(geometry).volume == pytest.approx(116.63908461722733, rel=1e-7)


def test_corrected_evidence_needs_explicit_issuer_link_not_defining_overlap():
    product = _take_inventory(channel(0.1))
    (candidate,) = product.accepted.candidate_set(FamilyId.POCKETS).candidates
    (published,) = product.result.section_recesses
    assert len(product.evidence.constituent_of(candidate)) == 4
    assert len(published.evidence.constituent_faces) == 3
    args = (candidate, (published,), product.context, product.evidence)
    assert _matching_recesses(*args) == ()
    provenance = {id(candidate.record): (published.body, published.evidence.constituent_faces)}
    assert _matching_recesses(*args, provenance) == (published,)
    assert _matching_recesses(*args, {id(object()): provenance[id(candidate.record)]}) == ()


def test_unserializable_channel_refuses_without_aborting_other_recognition(monkeypatch):
    def decline(_proof):
        raise ValueError("serialized cylindrical geometry exceeds displacement limit")

    monkeypatch.setattr(result_module, "cylindrical_channel_geometry", decline)
    product = _take_inventory(channel())
    assert product.result.section_recesses == ()
    assert len(product.result.section_recess_refusals) == 1
    assert product.result.section_recess_refusals[0].reason == "unsupported_support_geometry"
    assert product.result.holes  # the independently recognized bore survives
