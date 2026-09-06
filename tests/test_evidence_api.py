"""Contract tests for the supported within-run recognition-evidence view."""

from __future__ import annotations

import copy
import json
import pickle
from pathlib import Path

import pytest
from build123d import Box, Compound, Pos, RegularPolygon, Rot, extrude

import quiddity.evidence as evidence_module
from quiddity.evidence import (
    EVIDENCE_API_FORMAT,
    EVIDENCE_API_FORMAT_VERSION,
    AssociationMeasure,
    EvidenceApiManifestError,
    FaceRef,
    FeatureRef,
    GeometryAssociation,
    RecognitionEvidence,
    build_recognition_evidence,
    evidence_api_manifest,
    evidence_api_manifest_json,
)
from quiddity.result import build_raw_recognition_result

ROOT = Path(__file__).parents[1]
MANIFEST = ROOT / "src" / "quiddity" / "evidence_api.json"


def _two_equal_level_bodies() -> Compound:
    def step():
        return Box(60, 40, 10) + Pos(-15, 0, 10) * Box(30, 40, 10)

    first = step()
    return Compound([first, copy.deepcopy(first)])


def test_one_view_projects_the_existing_result_and_every_original_face() -> None:
    part = _two_equal_level_bodies()
    view = build_recognition_evidence(part)

    assert isinstance(view, RecognitionEvidence)
    assert view.result == build_raw_recognition_result(part)
    assert len(view.faces) == len(part.faces())
    resolved = [view.face(reference) for reference in view.faces]
    assert all(
        any(face.wrapped.IsSame(source.wrapped) for source in part.faces()) for face in resolved
    )


def test_equal_valued_occurrences_keep_distinct_feature_references() -> None:
    view = build_recognition_evidence(_two_equal_level_bodies())
    levels = tuple(
        reference for reference in view.features if view.family(reference) == "step_levels"
    )
    records = tuple(view.record(reference) for reference in levels)

    assert len(levels) == 2
    assert len({id(reference) for reference in levels}) == 2
    assert len(set(records)) == 1
    assert all(view.defining_faces(reference) for reference in levels)


def test_references_are_exactly_view_local_and_unforgeable() -> None:
    part = _two_equal_level_bodies()
    first = build_recognition_evidence(part)
    second = build_recognition_evidence(part)
    feature = first.features[0]
    face = next(iter(first.faces))

    with pytest.raises(ValueError, match="foreign, copied, forged, or stale"):
        second.record(feature)
    with pytest.raises(ValueError, match="foreign, copied, forged, or stale"):
        second.face(face)
    with pytest.raises(ValueError, match="foreign, copied, forged, or stale"):
        first.record(object.__new__(FeatureRef))
    with pytest.raises(ValueError, match="foreign, copied, forged, or stale"):
        first.face(object.__new__(FaceRef))
    with pytest.raises(TypeError, match="run-local"):
        pickle.dumps(feature)
    with pytest.raises(TypeError, match="run-local"):
        pickle.dumps(face)
    with pytest.raises(TypeError):
        copy.copy(feature)
    with pytest.raises(TypeError):
        copy.copy(face)
    with pytest.raises(TypeError, match="issued"):
        FeatureRef()
    with pytest.raises(TypeError, match="issued"):
        FaceRef()
    with pytest.raises(TypeError, match="created"):
        RecognitionEvidence()
    with pytest.raises(TypeError, match="FeatureRef"):
        first.record(face)  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="FeatureRef"):
        first.constituent_faces(face)  # type: ignore[arg-type]
    with pytest.raises(TypeError, match="FaceRef"):
        first.face(feature)  # type: ignore[arg-type]

    copied_feature = object.__new__(FeatureRef)
    object.__setattr__(
        copied_feature,
        "_FeatureRef__authority",
        object.__getattribute__(feature, "_FeatureRef__authority"),
    )
    copied_face = object.__new__(FaceRef)
    object.__setattr__(
        copied_face,
        "_FaceRef__authority",
        object.__getattribute__(face, "_FaceRef__authority"),
    )
    with pytest.raises(ValueError, match="foreign, copied, forged, or stale"):
        first.record(copied_feature)
    with pytest.raises(ValueError, match="foreign, copied, forged, or stale"):
        first.face(copied_face)


def test_defining_faces_resolve_to_the_exact_input_part() -> None:
    part = Box(10, 10, 5)
    view = build_recognition_evidence(part)

    for feature in view.features:
        for reference in view.defining_faces(feature):
            resolved = view.face(reference)
            assert any(resolved.wrapped.IsSame(face.wrapped) for face in part.faces())


def test_unmigrated_constituent_projection_defaults_exactly_to_defining() -> None:
    view = build_recognition_evidence(_two_equal_level_bodies())

    assert view.features
    for feature in view.features:
        assert view.constituent_faces(feature) == view.defining_faces(feature)
        assert view.constituent_faces(feature) <= view.faces


def test_public_constituent_projection_can_be_wider_than_defining() -> None:
    part = Box(100, 80, 10) + Pos(0, 0, 5) * extrude(RegularPolygon(20, 6), 30)
    view = build_recognition_evidence(part)
    (boss,) = tuple(
        feature for feature in view.features if view.family(feature) == "polygonal_bosses"
    )

    defining = view.defining_faces(boss)
    constituent = view.constituent_faces(boss)
    assert len(defining) == 6
    assert len(constituent) == 7
    assert defining < constituent <= view.faces


def test_public_prismatic_pocket_projection_includes_its_proved_floor() -> None:
    part = Box(80, 60, 20) - Pos(0, 0, 2) * extrude(RegularPolygon(10, 3), 30)
    view = build_recognition_evidence(part)
    (pocket,) = tuple(
        feature for feature in view.features if view.family(feature) == "section_recesses"
    )

    defining = view.defining_faces(pocket)
    constituent = view.constituent_faces(pocket)
    assert len(defining) == 3
    assert len(constituent) == 4
    assert defining < constituent <= view.faces


def test_no_accepted_features_leave_every_original_face_unassociated() -> None:
    part = Box(10, 20, 5)
    view = build_recognition_evidence(part)
    summary = view.association

    assert summary.face_count == AssociationMeasure(total=6, associated=0, unassociated=6)
    assert summary.face_count.ratio == 0.0
    assert summary.surface_area.associated == 0.0
    assert summary.surface_area.total == summary.surface_area.unassociated
    assert summary.surface_area.ratio == 0.0
    assert summary.families == ()
    assert summary.unassociated_faces == view.faces


def test_every_face_can_be_associated_without_inventing_background() -> None:
    part = extrude(RegularPolygon(10, 6), 20)
    view = build_recognition_evidence(part)
    summary = view.association

    assert summary.face_count == AssociationMeasure(total=8, associated=8, unassociated=0)
    assert summary.face_count.ratio == 1.0
    assert summary.surface_area.unassociated == 0.0
    assert summary.surface_area.associated == summary.surface_area.total
    assert summary.surface_area.ratio == 1.0
    assert len(summary.families) == 1
    assert summary.families[0].family == "polygonal_stock"
    assert summary.families[0].face_count == 8
    assert summary.families[0].surface_area == pytest.approx(part.area)
    assert summary.unassociated_faces == frozenset()


def test_overlapping_family_contributions_do_not_double_count_overall_association() -> None:
    part = Box(100, 80, 10) + Pos(0, 0, 5) * extrude(RegularPolygon(20, 6), 30)
    view = build_recognition_evidence(part)
    summary = view.association
    constituent_union = frozenset().union(
        *(view.constituent_faces(feature) for feature in view.features)
    )

    assert summary.face_count.associated == len(constituent_union)
    assert summary.face_count.total == (
        summary.face_count.associated + summary.face_count.unassociated
    )
    assert summary.surface_area.total == (
        summary.surface_area.associated + summary.surface_area.unassociated
    )
    assert sum(item.face_count for item in summary.families) > summary.face_count.associated
    assert summary.unassociated_faces == view.faces - constituent_union
    assert all(view.face(reference) for reference in summary.unassociated_faces)
    expected_families = tuple(dict.fromkeys(view.family(feature) for feature in view.features))
    assert tuple(item.family for item in summary.families) == expected_families
    for item in summary.families:
        family_faces = frozenset().union(
            *(
                view.constituent_faces(feature)
                for feature in view.features
                if view.family(feature) == item.family
            )
        )
        assert item.face_count == len(family_faces)
        assert item.surface_area == pytest.approx(
            sum(float(view.face(reference).area) for reference in family_faces)
        )


def test_compound_association_preserves_separate_body_faces_in_one_family_union() -> None:
    body = Box(100, 80, 10) + Pos(0, 0, 5) * extrude(RegularPolygon(20, 6), 30)
    first = Pos(-120, 0, 0) * body
    second = Pos(120, 0, 0) * copy.deepcopy(body)
    view = build_recognition_evidence(Compound([first, second]))

    bosses = tuple(
        feature for feature in view.features if view.family(feature) == "polygonal_bosses"
    )
    boss_summary = next(
        item for item in view.association.families if item.family == "polygonal_bosses"
    )
    assert len(bosses) == 2
    assert all(len(view.constituent_faces(feature)) == 7 for feature in bosses)
    assert boss_summary.face_count == 14
    assert view.association.face_count.total == 26
    assert view.association.face_count.associated == 18
    assert view.association.face_count.unassociated == 8


def test_empty_input_has_explicit_undefined_zero_denominator_ratios() -> None:
    view = build_recognition_evidence(Compound(children=[]))

    assert view.association == GeometryAssociation(
        face_count=AssociationMeasure(total=0, associated=0, unassociated=0),
        surface_area=AssociationMeasure(total=0.0, associated=0.0, unassociated=0.0),
        families=(),
        unassociated_faces=frozenset(),
    )
    assert view.association.face_count.ratio is None
    assert view.association.surface_area.ratio is None


def test_projection_preserves_inventory_order_and_transformed_face_binding() -> None:
    def step():
        return Box(60, 40, 10) + Pos(-15, 0, 10) * Box(30, 40, 10)

    left = Pos(-70, 0, 0) * step()
    right = Pos(70, 0, 0) * step()
    forward = build_recognition_evidence(Compound([left, right]))
    reverse = build_recognition_evidence(Compound([right, left]))

    def signature(view: RecognitionEvidence):
        return tuple(
            (view.family(feature), view.record(feature).to_dict()) for feature in view.features
        )

    assert signature(forward) == signature(reverse)

    rotated_part = Rot(90, 0, 0) * step()
    rotated = build_recognition_evidence(rotated_part)
    assert len(rotated.faces) == len(rotated_part.faces())
    for feature in rotated.features:
        assert rotated.defining_faces(feature) <= rotated.faces


def test_manifest_is_closed_deterministic_isolated_and_independently_versioned() -> None:
    expected = json.loads(MANIFEST.read_text(encoding="utf-8"))
    first = evidence_api_manifest()

    assert first == expected == evidence_api_manifest()
    assert evidence_api_manifest_json() == MANIFEST.read_text(encoding="utf-8")
    assert first["format"] == EVIDENCE_API_FORMAT
    assert first["format_version"] == EVIDENCE_API_FORMAT_VERSION
    first_api = first["api"]
    assert isinstance(first_api, dict)
    assert first_api["symbols"] == sorted(evidence_module.__all__)
    first_api["symbols"] = []
    assert evidence_api_manifest()["api"] != first_api
    for invalid in (True, 0, 2):
        with pytest.raises(EvidenceApiManifestError, match="unsupported requested"):
            evidence_api_manifest(format_version=invalid)


def test_manifest_validation_rejects_each_closed_contract_layer() -> None:
    valid = json.loads(MANIFEST.read_text(encoding="utf-8"))
    invalid_manifests: list[tuple[object, str]] = [
        (None, "closed shape"),
        ({**valid, "unexpected": True}, "closed shape"),
        ({**valid, "format": "other"}, "format is unsupported"),
        (
            {**valid, "package": {"name": "other", "version": valid["package"]["version"]}},
            "package identity",
        ),
        ({**valid, "api": {**valid["api"], "unexpected": True}}, "declaration"),
        ({**valid, "api": {**valid["api"], "major": 2}}, "malformed"),
    ]

    for manifest, message in invalid_manifests:
        with pytest.raises(EvidenceApiManifestError, match=message):
            evidence_module._validate_manifest(manifest)
