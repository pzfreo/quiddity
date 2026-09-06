from __future__ import annotations

import json
import math
from dataclasses import replace
from pathlib import Path
from types import SimpleNamespace

import pytest
from build123d import (
    Box,
    BuildPart,
    BuildSketch,
    Compound,
    Cylinder,
    Plane,
    Polygon,
    Pos,
    Rot,
    export_step,
    extrude,
    import_step,
)

from quiddity import (
    OpenSectionProfile,
    PassageSectionVertex,
    SectionEnd,
    SectionRecess,
    SectionRecessEnds,
    build_section_recess_document,
    recognise_section_recesses,
)
from quiddity._section_recess import (
    SectionRecessClassification,
)
from quiddity._section_recess_geometry import project_section_recess_geometry
from quiddity._sections import (
    BodyRefIssuer,
    LocalFrame,
    PlanarSection,
    SectionEnds,
    SectionOccurrence,
    SectionVertex,
)
from tools._legacy_recognition import (
    build_raw_recognition_result,
)


def _obround(*, straight: float = 12, width: float = 6, depth: float = 8):
    radius = width / 2
    return (
        Box(straight, width, depth)
        + Pos(-straight / 2, 0, 0) * Cylinder(radius, depth)
        + Pos(straight / 2, 0, 0) * Cylinder(radius, depth)
    )


def _blind_pocket(*, angle: float = 30):
    return Box(60, 50, 12) - Pos(0, 0, 4) * Rot(0, 0, angle) * _obround()


@pytest.mark.parametrize("placement", [Rot(), Rot(180, 0, 0), Rot(17, 31, 43)])
@pytest.mark.parametrize("obstruction", [
    Pos(0, 0, 3) * Box(2, 10, 1),  # bridge through the straight middle
    Pos(9, 0, 3) * Box(2, 1, 1),  # intrudes only into the curved cap, outside its chord
    Pos(0, 0, 6.5) * Box(2, 10, 1),  # blocks the mouth without entering the run
])
def test_obround_rejects_same_body_run_and_mouth_obstructions(placement, obstruction):
    base = _blind_pocket(angle=0)
    assert len(build_section_recess_document(placement * base).occurrences) == 1
    blocked = placement * (base + obstruction)
    assert blocked.is_valid and len(blocked.solids()) == 1
    assert not any(record.classification.section_shape == "obround"
                   for record in build_section_recess_document(blocked).occurrences)


def test_obround_material_probe_uses_only_the_owning_body():
    part = Compound([_blind_pocket(angle=0), Pos(0, 0, 3) * Box(2, 2, 1)])
    document = build_section_recess_document(part)
    assert sum(record.classification.section_shape == "obround"
               for record in document.occurrences) == 1


def test_obround_probe_contains_the_complete_semicircular_ends():
    from quiddity._section_recess_geometry import _obround_prism

    probe = _obround_prism((0, 0, 1), (-6, 0, 0), (6, 0, 0), 3, 0, 6)
    assert probe.volume == pytest.approx((12 * 6 + math.pi * 3**2) * 6)


def test_obround_kernel_probe_failure_refuses(monkeypatch):
    import quiddity._section_recess_geometry as implementation

    def failed(*_args):
        raise RuntimeError("kernel boolean failed")

    monkeypatch.setattr(implementation, "_material_fraction", failed)
    assert build_section_recess_document(_blind_pocket()).occurrences == ()


@pytest.mark.parametrize("fractions", [(1.0,), (0.0, 1.0), (0.0, 0.0, 0.0)])
def test_obround_requires_empty_run_open_mouth_and_complete_backing(monkeypatch, fractions):
    import quiddity._section_recess_geometry as implementation
    from quiddity._adjacency import FaceGraph

    graph = FaceGraph(_blind_pocket(angle=0))
    floor = next(node for node in graph.nodes
                 if implementation._one_obround_candidate(graph, node) is not None)
    remaining = iter(fractions)
    monkeypatch.setattr(implementation, "_material_fraction", lambda *_args: next(remaining))
    assert implementation._one_obround_candidate(graph, floor) is None
    assert list(remaining) == []


def _polygonal_cutter(points, *, depth: float = 8):
    with BuildPart() as cutter:
        with BuildSketch(Plane.XY):
            Polygon(*points)
        extrude(amount=depth)
    return cutter.part


def _polygonal_pocket(points, *, placement=None):
    if placement is None:
        placement = Rot(17, 31, 43)
    raw = Box(60, 50, 12) - Pos(0, 0, 4) * _polygonal_cutter(points)
    return placement * raw


def test_section_recess_emits_reconstructible_indexed_json() -> None:
    document = build_section_recess_document(_blind_pocket())

    assert [body.index for body in document.bodies] == [0]
    assert [face.index for face in document.faces] == list(range(11))
    (occurrence,) = document.occurrences
    assert occurrence.index == 0
    assert occurrence.body == 0
    assert occurrence.classification.to_dict() == {
        "feature_kind": "pocket",
        "section_shape": "obround",
    }
    assert len(occurrence.evidence.defining_faces) == 4
    assert len(occurrence.evidence.constituent_faces) == 5
    assert occurrence.geometry.run_interval == (0.0, 6.0)
    assert occurrence.geometry.ends.low.condition == "capped"
    assert occurrence.geometry.ends.high.condition == "open"

    boundary = occurrence.geometry.profile.boundary
    section = PlanarSection(tuple(SectionVertex(vertex.point, vertex.bulge) for vertex in boundary))
    assert section.centroid == pytest.approx((0.0, 0.0), abs=8e-4)
    assert section.area == pytest.approx(72 + 9 * math.pi, abs=2e-2)
    json.dumps(document.to_dict())


def test_trapezoid_is_not_classified_as_rectangle() -> None:
    part = _polygonal_pocket(((-5, -3), (5, -3), (3, 3), (-3, 3)))
    records = build_section_recess_document(part).occurrences
    assert records
    assert all(record.classification.section_shape == "polygonal" for record in records)


@pytest.mark.parametrize("chamfer", [0.0002, 0.0004, 0.0006])
def test_micro_chamfer_projection_preserves_occurrence_and_evidence(chamfer, tmp_path):
    with BuildSketch() as sketch:
        Polygon((-6, -4), (6, -4), (6, 4 - chamfer), (6 - chamfer, 4), (-6, 4), align=None)
    part = Box(30, 20, 10) - Pos(0, 0, 1) * extrude(sketch.sketch, amount=6)
    path = tmp_path / "micro-chamfer.step"
    export_step(part, path)
    for source in (part, import_step(path)):
        result = build_raw_recognition_result(source)
        (legacy,) = result.prismatic_pockets
        (unified,) = result.section_recesses
        assert legacy.sides == 5
        assert len(unified.evidence.defining_faces) == 5
        assert len(unified.evidence.constituent_faces) == 6
        assert unified.classification.section_shape == "polygonal"
        boundary = unified.geometry.profile.boundary
        assert len(boundary) == (4 if chamfer < 0.0005 else 5)
        assert len({vertex.point for vertex in boundary}) == len(boundary)
        assert unified.geometry.run_interval == (1.0, 5.0)


def test_section_recess_output_matches_committed_golden() -> None:
    document = build_section_recess_document(_blind_pocket())
    actual = document.to_dict()
    occurrence = actual["occurrences"][0]
    projection = {
        "schema_version": actual["schema_version"],
        "reference_scope": actual["reference_scope"],
        "body_indices": [body["index"] for body in actual["bodies"]],
        "face_count": len(actual["faces"]),
        "occurrences": [
            {
                "index": occurrence["index"],
                "body": occurrence["body"],
                "geometry_type": occurrence["geometry"]["type"],
                "profile_closure": occurrence["geometry"]["profile"]["closure"],
                "end_conditions": [
                    occurrence["geometry"]["ends"]["low"]["condition"],
                    occurrence["geometry"]["ends"]["high"]["condition"],
                ],
                "classification": occurrence["classification"],
                "defining_face_count": len(occurrence["evidence"]["defining_faces"]),
                "constituent_face_count": len(occurrence["evidence"]["constituent_faces"]),
            }
        ],
    }
    expected_path = Path(__file__).with_name("section_recess_expected.json")

    assert projection == json.loads(expected_path.read_text(encoding="utf-8"))


@pytest.mark.parametrize(
    "placement",
    [Rot(90, 0, 0), Rot(0, 90, 0), Rot(17, 31, 43) * Pos(11, -7, 5)],
)
def test_section_recess_is_covariant_under_rigid_presentation(placement) -> None:
    document = build_section_recess_document(placement * _blind_pocket())

    (occurrence,) = document.occurrences
    span = occurrence.geometry.run_interval[1] - occurrence.geometry.run_interval[0]
    assert span == pytest.approx(6.0, abs=2e-3)
    assert occurrence.geometry.profile.closure == "closed"
    assert tuple(vertex.bulge for vertex in occurrence.geometry.profile.boundary).count(1.0) == 2


def test_section_recess_does_not_publish_a_boss_as_a_pocket() -> None:
    boss = Box(60, 50, 6) + Pos(0, 0, 7) * Rot(0, 0, 30) * _obround()

    assert build_section_recess_document(boss).occurrences == ()


def test_section_recess_is_a_public_aggregate_family() -> None:
    part = Rot(17, 31, 43) * _blind_pocket()

    direct = recognise_section_recesses(part)
    aggregate = build_raw_recognition_result(part)

    assert direct
    assert all(isinstance(record, SectionRecess) for record in direct)
    assert aggregate.section_recesses == tuple(direct)


def test_document_projects_completed_aggregate_inventory(monkeypatch) -> None:
    import quiddity.result as result_module

    part = _blind_pocket()
    (record,) = recognise_section_recesses(part)
    sentinel = replace(record, index=7)

    def completed_inventory(supplied):
        assert supplied is part
        return SimpleNamespace(section_recesses=(sentinel,), section_recess_refusals=(),
                               section_recess_patterns=())

    monkeypatch.setattr(
        result_module,
        "build_raw_recognition_result",
        completed_inventory,
    )

    document = build_section_recess_document(part)

    assert document.occurrences == (replace(sentinel, index=0),)


def test_unified_contract_admits_only_decided_profile_and_end_combinations() -> None:
    (base,) = recognise_section_recesses(_blind_pocket())
    closed = base.geometry.profile
    open_profile = OpenSectionProfile(
        "open",
        (
            PassageSectionVertex((-2.0, 1.0), 0.0),
            PassageSectionVertex((-2.0, -1.0), 0.0),
            PassageSectionVertex((2.0, -1.0), 0.0),
            PassageSectionVertex((2.0, 1.0), 0.0),
        ),
        ((2.0, 1.0), (-2.0, 1.0)),
    )
    one_cap = SectionRecessEnds(SectionEnd("capped"), SectionEnd("open"))
    no_caps = SectionRecessEnds(SectionEnd("open"), SectionEnd("open"))
    two_caps = SectionRecessEnds(SectionEnd("capped"), SectionEnd("capped"))

    def occurrence(kind, profile, ends):
        return replace(
            base,
            geometry=replace(base.geometry, profile=profile, ends=ends),
            classification=SectionRecessClassification(kind, "rectangular"),
        )

    assert occurrence("pocket", closed, one_cap)
    assert occurrence("edge_open_recess", open_profile, one_cap)
    assert occurrence("passage", closed, no_caps)
    assert occurrence("channel", open_profile, no_caps)
    for kind, profile, ends in (
        ("pocket", open_profile, one_cap),
        ("edge_open_recess", closed, one_cap),
        ("passage", closed, one_cap),
        ("pocket", closed, two_caps),
        ("channel", closed, no_caps),
        ("channel", open_profile, one_cap),
    ):
        with pytest.raises(ValueError, match="profile closure and end topology"):
            occurrence(kind, profile, ends)


def test_open_profile_refuses_an_implied_or_misdirected_closure() -> None:
    vertices = (
        PassageSectionVertex((-2.0, 1.0), 0.0),
        PassageSectionVertex((-2.0, -1.0), 0.0),
        PassageSectionVertex((2.0, -1.0), 0.0),
        PassageSectionVertex((2.0, 1.0), 0.0),
    )

    with pytest.raises(ValueError, match="final open-profile vertex"):
        OpenSectionProfile(
            "open",
            (*vertices[:-1], PassageSectionVertex(vertices[-1].point, 0.5)),
            ((2.0, 1.0), (-2.0, 1.0)),
        )
    with pytest.raises(ValueError, match="physical chain end"):
        OpenSectionProfile("open", vertices, ((-2.0, 1.0), (2.0, 1.0)))


@pytest.mark.parametrize(
    ("shape", "points"),
    [
        ("triangular", ((-4.0, -2.0), (4.0, -2.0), (0.0, 4.0))),
        ("rectangular", ((-4.0, -2.0), (4.0, -2.0), (4.0, 2.0), (-4.0, 2.0))),
        (
            "hexagonal",
            ((-4.0, 0.0), (-2.0, -3.0), (2.0, -3.0), (4.0, 0.0), (2.0, 3.0), (-2.0, 3.0)),
        ),
    ],
)
def test_unified_contract_projects_free_axis_polygonal_sections(shape, points) -> None:
    issuer = BodyRefIssuer()
    occurrence = SectionOccurrence(
        issuer.issue(),
        LocalFrame.canonical((1.0, 2.0, 3.0), (0.0, 0.0, 0.0)),
        (-2.0, 5.0),
        PlanarSection(tuple(SectionVertex(point) for point in points)),
        SectionEnds(True, False),
    )

    geometry = project_section_recess_geometry(occurrence, body_refs=issuer)
    classification = SectionRecessClassification("pocket", shape)

    assert geometry.type == "section_recess"
    assert geometry.run_interval == (-2.0, 5.0)
    assert all(vertex.bulge == 0.0 for vertex in geometry.profile.boundary)
    assert classification.section_shape == shape


@pytest.mark.parametrize(
    ("shape", "points"),
    [
        ("triangular", ((-4.0, -3.0), (4.0, -3.0), (0.0, 5.0))),
        ("rectangular", ((-5.0, -3.0), (5.0, -3.0), (5.0, 3.0), (-5.0, 3.0))),
        (
            "hexagonal",
            ((-5.0, 0.0), (-2.5, -4.0), (2.5, -4.0), (5.0, 0.0), (2.5, 4.0), (-2.5, 4.0)),
        ),
    ],
)
def test_free_frame_floor_proof_recognises_polygonal_pockets(shape, points) -> None:
    document = build_section_recess_document(_polygonal_pocket(points))

    (occurrence,) = document.occurrences
    assert occurrence.classification.section_shape == shape
    assert len(occurrence.evidence.defining_faces) == len(points)
    assert len(occurrence.evidence.constituent_faces) == len(points) + 1


def test_polygonal_proof_is_stable_after_step_round_trip(tmp_path) -> None:
    path = tmp_path / "oriented-triangle.step"
    export_step(_polygonal_pocket(((-4, -3), (4, -3), (0, 5))), path)

    (occurrence,) = build_section_recess_document(import_step(path)).occurrences

    assert occurrence.classification.section_shape == "triangular"


def test_unified_inventory_classifies_through_cut_and_rejects_boss() -> None:
    points = ((-4.0, -3.0), (4.0, -3.0), (0.0, 5.0))
    through = Box(60, 50, 12) - Pos(0, 0, -7) * _polygonal_cutter(points, depth=14)
    boss = Box(60, 50, 6) + Pos(0, 0, 7) * _polygonal_cutter(points)

    through_document = build_section_recess_document(through)

    assert [item.classification.feature_kind for item in through_document.occurrences] == [
        "passage"
    ]
    assert build_section_recess_document(boss).occurrences == ()


def test_passage_projection_preserves_geometry_evidence_and_body() -> None:
    points = ((-4.0, -3.0), (4.0, -3.0), (0.0, 5.0))
    untouched = Pos(-100, 0, 0) * Box(10, 10, 10)
    passage = Pos(100, 0, 0) * (
        Box(60, 50, 12) - Pos(0, 0, -7) * _polygonal_cutter(points, depth=14)
    )

    document = build_section_recess_document(Compound([untouched, passage]))

    (record,) = document.occurrences
    assert record.body == 1
    assert record.classification.to_dict() == {
        "feature_kind": "passage",
        "section_shape": "triangular",
    }
    assert record.geometry.profile.closure == "closed"
    assert record.geometry.ends.low.condition == "open"
    assert record.geometry.ends.high.condition == "open"
    assert record.evidence.defining_faces
    assert set(record.evidence.defining_faces) <= set(record.evidence.constituent_faces)


def test_prismatic_pocket_projection_uses_one_unified_occurrence() -> None:
    points = ((-4.0, -3.0), (4.0, -3.0), (0.0, 5.0))
    part = _polygonal_pocket(points, placement=Pos(0, 0, 0))

    result = build_raw_recognition_result(part)

    assert len(result.prismatic_pockets) == 1
    assert len(result.section_recesses) == 1
    (record,) = result.section_recesses
    assert record.classification.to_dict() == {
        "feature_kind": "pocket",
        "section_shape": "triangular",
    }
    assert record.geometry.profile.closure == "closed"
    assert {record.geometry.ends.low.condition, record.geometry.ends.high.condition} == {
        "capped",
        "open",
    }


def test_equal_polygonal_pockets_on_separate_bodies_keep_ownership() -> None:
    points = ((-4.0, -3.0), (4.0, -3.0), (0.0, 5.0))
    first = _polygonal_pocket(points)
    second = Pos(100, 0, 0) * _polygonal_pocket(points)

    document = build_section_recess_document(Compound([first, second]))

    assert len(document.occurrences) == 2
    assert {occurrence.body for occurrence in document.occurrences} == {0, 1}


def test_document_preserves_unrecognised_body_in_reference_roster() -> None:
    untouched = Pos(-100, 0, 0) * Box(10, 10, 10)
    recognised = Pos(100, 0, 0) * _blind_pocket()
    part = Compound([untouched, recognised])

    document = build_section_recess_document(part)
    direct = recognise_section_recesses(part)

    assert [body.index for body in document.bodies] == [0, 1]
    assert [occurrence.body for occurrence in document.occurrences] == [1]
    assert [occurrence.body for occurrence in direct] == [1]
