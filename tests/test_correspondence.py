"""Authored revised-geometry corpus for ADR 0026 correspondence receipts."""

from __future__ import annotations

import copy
import json

import pytest
from build123d import Box, BuildPart, Compound, Cylinder, Locations, Mode

from quiddity.correspondence import (
    CORRESPONDENCE_API_FORMAT,
    CORRESPONDENCE_API_FORMAT_VERSION,
    CorrespondenceReceipt,
    CorrespondenceReceiptError,
    ReceiptSubjectKind,
    ResolutionStatus,
    correspondence_api_manifest,
    correspondence_api_manifest_json,
    issue_correspondence_receipt,
    resolve_correspondence_receipts,
)
from quiddity.evidence import FaceRef, FeatureRef, build_recognition_evidence


def _plate_with_holes(
    holes: tuple[tuple[float, float, float], ...], *, size: tuple[float, float] = (80, 50)
):
    with BuildPart() as part:
        Box(*size, 10)
        for x, y, diameter in holes:
            with Locations((x, y, 0)):
                Cylinder(diameter / 2, 10, mode=Mode.SUBTRACT)
    return part.part


def _hole_features(view) -> tuple[FeatureRef, ...]:
    return tuple(feature for feature in view.features if view.family(feature) == "holes")


def _face_at(view, *, z: float) -> FaceRef:
    matches = tuple(
        reference
        for reference in view.faces
        if abs(float(view.face(reference).center().Z) - z) < 1e-7
        and abs(float(view.face(reference).normal_at().Z)) > 0.99
    )
    assert len(matches) == 1
    return matches[0]


def test_receipt_round_trip_is_closed_strict_json() -> None:
    view = build_recognition_evidence(_plate_with_holes(((0, 0, 6),)))
    receipt = issue_correspondence_receipt(view, _hole_features(view)[0], lineage="part-42")

    restored = CorrespondenceReceipt.from_json(receipt.to_json())

    assert restored.lineage == "part-42"
    assert restored.subject_kind is ReceiptSubjectKind.FEATURE
    assert restored.strategy == "analytic-v1"
    assert restored.format_version == 1
    assert restored.to_dict() == receipt.to_dict()
    assert set(restored.to_dict()) == {
        "format",
        "format_version",
        "lineage",
        "payload",
        "strategy",
        "subject_kind",
    }


def test_plain_datum_face_survives_an_added_inner_loop() -> None:
    before = build_recognition_evidence(Box(80, 50, 10))
    receipt = issue_correspondence_receipt(before, _face_at(before, z=5), lineage="plate")
    after = build_recognition_evidence(_plate_with_holes(((0, 0, 6),)))

    (resolved,) = resolve_correspondence_receipts((receipt,), after, lineage="plate")

    assert resolved.status is ResolutionStatus.RESOLVED
    assert resolved.feature is None
    assert resolved.faces == {_face_at(after, z=5)}


def test_explicit_planar_face_set_resolves_as_one_subject() -> None:
    before = build_recognition_evidence(Box(80, 50, 10))
    subject = frozenset((_face_at(before, z=-5), _face_at(before, z=5)))
    receipt = issue_correspondence_receipt(before, subject, lineage="plate")
    after = build_recognition_evidence(_plate_with_holes(((0, 0, 6),)))

    (resolved,) = resolve_correspondence_receipts((receipt,), after, lineage="plate")

    assert resolved.status is ResolutionStatus.RESOLVED
    assert resolved.faces == {_face_at(after, z=-5), _face_at(after, z=5)}


@pytest.mark.parametrize(
    ("before_hole", "after_hole"),
    [
        ((0, 0, 6), (0, 0, 8)),  # resized at one proved opening/axis
        ((0, 0, 6), (12, 3, 6)),  # moved with one unique retained machining spec
    ],
)
def test_unique_resized_or_moved_hole_resolves(before_hole, after_hole) -> None:
    before = build_recognition_evidence(_plate_with_holes((before_hole,)))
    receipt = issue_correspondence_receipt(
        before, _hole_features(before)[0], lineage="edited-plate"
    )
    after = build_recognition_evidence(_plate_with_holes((after_hole,)))

    (resolved,) = resolve_correspondence_receipts((receipt,), after, lineage="edited-plate")

    assert resolved.status is ResolutionStatus.RESOLVED
    assert resolved.feature is _hole_features(after)[0]
    assert resolved.faces == frozenset()


def test_exact_hole_wins_when_an_equal_spec_hole_was_added() -> None:
    before = build_recognition_evidence(_plate_with_holes(((0, 0, 6),)))
    receipt = issue_correspondence_receipt(before, _hole_features(before)[0], lineage="plate")
    after = build_recognition_evidence(_plate_with_holes(((0, 0, 6), (20, 0, 6))))

    (resolved,) = resolve_correspondence_receipts((receipt,), after, lineage="plate")

    expected = next(
        feature
        for feature in _hole_features(after)
        if tuple(after.record(feature).location) == (0.0, 0.0, 5.0)
    )
    assert resolved.status is ResolutionStatus.RESOLVED
    assert resolved.feature is expected


def test_symmetric_hole_alternatives_refuse_instead_of_using_distance_or_order() -> None:
    before = build_recognition_evidence(_plate_with_holes(((0, 0, 6),)))
    receipt = issue_correspondence_receipt(before, _hole_features(before)[0], lineage="plate")
    after = build_recognition_evidence(_plate_with_holes(((-15, 0, 6), (15, 0, 6))))

    (resolved,) = resolve_correspondence_receipts((receipt,), after, lineage="plate")

    assert resolved.status is ResolutionStatus.AMBIGUOUS
    assert resolved.feature is None
    assert resolved.faces == frozenset()


def test_coincident_equal_face_alternatives_refuse() -> None:
    one = Box(30, 20, 10)
    before = build_recognition_evidence(one)
    receipt = issue_correspondence_receipt(before, _face_at(before, z=5), lineage="bodies")
    after = build_recognition_evidence(Compound([one, copy.deepcopy(one)]))

    (resolved,) = resolve_correspondence_receipts((receipt,), after, lineage="bodies")

    assert resolved.status is ResolutionStatus.AMBIGUOUS


def test_global_batch_does_not_assign_one_current_hole_twice() -> None:
    before = build_recognition_evidence(_plate_with_holes(((-10, 0, 6), (10, 0, 6))))
    receipts = tuple(
        issue_correspondence_receipt(before, feature, lineage="plate")
        for feature in _hole_features(before)
    )
    after = build_recognition_evidence(_plate_with_holes(((0, 0, 6),)))

    result = resolve_correspondence_receipts(receipts, after, lineage="plate")

    assert [item.status for item in result] == [
        ResolutionStatus.AMBIGUOUS,
        ResolutionStatus.AMBIGUOUS,
    ]


def test_removed_subject_and_foreign_lineage_are_typed() -> None:
    before = build_recognition_evidence(_plate_with_holes(((0, 0, 6),)))
    receipt = issue_correspondence_receipt(before, _hole_features(before)[0], lineage="plate")
    after = build_recognition_evidence(Box(80, 50, 10))

    (missing,) = resolve_correspondence_receipts((receipt,), after, lineage="plate")
    (foreign,) = resolve_correspondence_receipts((receipt,), after, lineage="another-part")

    assert missing.status is ResolutionStatus.MISSING
    assert foreign.status is ResolutionStatus.INCOMPATIBLE


def test_malformed_and_modified_receipts_fail_before_matching() -> None:
    view = build_recognition_evidence(_plate_with_holes(((0, 0, 6),)))
    receipt = issue_correspondence_receipt(view, _hole_features(view)[0], lineage="plate")
    encoded = receipt.to_dict()
    encoded["payload"] = str(encoded["payload"])[:-2] + "AA"

    with pytest.raises(CorrespondenceReceiptError, match="integrity|encoding"):
        CorrespondenceReceipt.from_dict(encoded)
    with pytest.raises(CorrespondenceReceiptError, match="closed shape"):
        CorrespondenceReceipt.from_dict({**receipt.to_dict(), "unknown": True})
    with pytest.raises(TypeError, match="issued"):
        CorrespondenceReceipt()


def test_correspondence_manifest_matches_the_installed_contract() -> None:
    manifest = correspondence_api_manifest()

    assert manifest["format"] == CORRESPONDENCE_API_FORMAT
    assert manifest["format_version"] == CORRESPONDENCE_API_FORMAT_VERSION
    assert json.loads(correspondence_api_manifest_json()) == manifest
