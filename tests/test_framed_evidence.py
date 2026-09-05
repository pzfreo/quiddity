"""Contract tests for one-run framed accepted-occurrence evidence."""

from __future__ import annotations

import copy
from dataclasses import FrozenInstanceError, fields
from typing import cast

import pytest
from build123d import Axis, Box, Compound, Cylinder, Pos, RegularPolygon, extrude

import quiddity.evidence as evidence_module
import quiddity.frames as frames
from quiddity._typing import Part
from quiddity.evidence import (
    FaceRef,
    FramedEvidenceRefusalReason,
    FramedRecognitionEvidence,
    RefusedFramedEvidence,
)
from quiddity.frames import (
    FramedRecognitionResult,
    FrameGauge,
    PreparedFramedPart,
    RefusedPartFrame,
    build_framed_recognition_evidence,
    prepare_framed_part,
)


def _full_gauge_part():
    return Box(10, 20, 30) + Pos(9, 18, 28) * Box(2, 3, 4) - Pos(3, 4, 0) * Cylinder(1, 30)


def _axial_part():
    return Cylinder(10, 30) + Pos(0, 0, 30) * Cylinder(7, 10)


@pytest.mark.parametrize(
    ("source", "gauge"),
    [
        (_full_gauge_part(), FrameGauge.FULL),
        (Box(10, 20, 30), FrameGauge.ORTHOGONAL),
        (_axial_part(), FrameGauge.AXIAL),
    ],
)
def test_framed_evidence_maps_every_local_face_exactly_to_its_caller_partner(
    source, gauge
) -> None:
    caller = Pos(13, -7, 5) * source.rotate(Axis((0, 0, 0), (1, 1, 0)), 37)
    framed = build_framed_recognition_evidence(cast(Part, caller), rotational=True)

    assert isinstance(framed, FramedRecognitionEvidence)
    assert framed.frame.gauge is gauge
    assert framed.caller_part is caller
    assert len(framed.faces) == len(caller.faces()) == len(framed.part.faces())
    assert framed.association.face_count.total == len(framed.faces)
    assert framed.association.surface_area.total == pytest.approx(
        sum(float(face.area) for face in caller.faces())
    )
    for reference in framed.faces:
        local = framed.face(reference)
        original = framed.caller_face(reference)
        assert local.wrapped.IsPartner(original.wrapped)
        assert not local.wrapped.IsSame(original.wrapped)
        assert sum(original.wrapped.IsSame(face.wrapped) for face in caller.faces()) == 1
        assert sum(local.wrapped.IsSame(face.wrapped) for face in framed.part.faces()) == 1


def test_prepared_evidence_preserves_occurrences_body_ownership_and_constituent_invariant() -> None:
    body = Box(100, 80, 10) + Pos(0, 0, 5) * extrude(RegularPolygon(20, 6), 30)
    caller = Compound(
        [
            Pos(-120, 0, 0) * body,
            Pos(120, 0, 0) * copy.deepcopy(body),
        ]
    ).rotate(Axis.X, 30)
    prepared = prepare_framed_part(cast(Part, caller))
    assert isinstance(prepared, PreparedFramedPart)

    framed = prepared.recognise_evidence()

    assert isinstance(framed, FramedRecognitionEvidence)
    bosses = tuple(
        feature for feature in framed.features if framed.family(feature) == "polygonal_bosses"
    )
    assert len(bosses) == 2
    assert len({id(feature) for feature in bosses}) == 2
    for feature in framed.features:
        assert framed.defining_faces(feature) <= framed.constituent_faces(feature) <= framed.faces
    for feature in bosses:
        caller_faces = tuple(framed.caller_face(face) for face in framed.constituent_faces(feature))
        assert len(caller_faces) == 7
        assert all(
            not left.wrapped.IsSame(right.wrapped)
            for index, left in enumerate(caller_faces)
            for right in caller_faces[index + 1 :]
        )


def test_caller_mapping_distinguishes_located_occurrences_that_share_one_tshape() -> None:
    body = Box(10, 20, 30)
    caller = Compound([Pos(-20, 0, 0) * body, Pos(20, 0, 0) * body]).rotate(Axis.X, 30)
    caller_faces = caller.faces()
    assert any(
        left.wrapped.IsPartner(right.wrapped) and not left.wrapped.IsSame(right.wrapped)
        for index, left in enumerate(caller_faces)
        for right in caller_faces[index + 1 :]
    )

    prepared = prepare_framed_part(cast(Part, caller))
    assert isinstance(prepared, PreparedFramedPart)
    assert prepared._placement is not None
    framed = prepared.recognise_evidence()

    assert isinstance(framed, FramedRecognitionEvidence)
    location = prepared._placement
    resolved = tuple(framed.caller_face(reference) for reference in framed.faces)
    assert len(resolved) == len(caller_faces)
    assert all(
        framed.face(reference).wrapped.IsSame(
            (location * framed.caller_face(reference)).wrapped
        )
        for reference in framed.faces
    )
    assert all(
        sum(face.wrapped.IsSame(source.wrapped) for face in resolved) == 1
        for source in caller_faces
    )


def test_prepared_evidence_runs_one_aggregate_and_never_calls_the_raw_entrypoint(
    monkeypatch,
) -> None:
    caller = Pos(13, -7, 5) * _axial_part().rotate(Axis.X, 37)
    prepared = prepare_framed_part(cast(Part, caller))
    assert isinstance(prepared, PreparedFramedPart)
    original = frames._take_inventory
    calls = []

    def counted(part, *, cylinders=None, rotational=False):
        calls.append((part, cylinders, rotational))
        return original(part, cylinders=cylinders, rotational=rotational)

    def forbidden(*_args, **_kwargs):
        raise AssertionError("framed evidence must not call the raw evidence entrypoint")

    monkeypatch.setattr(frames, "_take_inventory", counted)
    monkeypatch.setattr(evidence_module, "build_recognition_evidence", forbidden)

    framed = prepared.recognise_evidence(rotational=True)

    assert isinstance(framed, FramedRecognitionEvidence)
    assert len(calls) == 1
    assert calls[0][0] is prepared.part
    assert calls[0][2] is True
    assert framed.result.rotational is True


def test_direct_framed_evidence_prepares_cylinders_once(monkeypatch) -> None:
    caller = Pos(13, -7, 5) * _axial_part().rotate(Axis.X, 37)
    original = frames.analyse_cylinders
    calls = []

    def counted(part):
        calls.append(part)
        return original(part)

    monkeypatch.setattr(frames, "analyse_cylinders", counted)

    framed = build_framed_recognition_evidence(cast(Part, caller), rotational=True)

    assert isinstance(framed, FramedRecognitionEvidence)
    assert calls == [framed.part]


def test_mapping_refuses_before_recognition_when_prepared_value_lacks_caller_authority(
    monkeypatch,
) -> None:
    prepared = prepare_framed_part(Box(10, 20, 30))
    assert isinstance(prepared, PreparedFramedPart)
    detached = PreparedFramedPart(prepared.frame, prepared.part, prepared.cylinders)

    def forbidden(*_args, **_kwargs):
        raise AssertionError("mapping refusal must precede aggregate recognition")

    monkeypatch.setattr(frames, "_take_inventory", forbidden)

    assert detached.recognise_evidence() == RefusedFramedEvidence(
        FramedEvidenceRefusalReason.CALLER_FACE_MAPPING_UNAVAILABLE
    )


def test_non_bijective_partner_mapping_refuses_without_order_or_coordinate_fallback(
    monkeypatch,
) -> None:
    prepared = prepare_framed_part(Box(10, 20, 30))
    assert isinstance(prepared, PreparedFramedPart)
    monkeypatch.setattr(frames, "_caller_face_bijection", lambda *_args: None)

    def forbidden(*_args, **_kwargs):
        raise AssertionError("mapping refusal must precede aggregate recognition")

    monkeypatch.setattr(frames, "_take_inventory", forbidden)

    assert prepared.recognise_evidence() == RefusedFramedEvidence(
        FramedEvidenceRefusalReason.CALLER_FACE_MAPPING_UNAVAILABLE
    )


@pytest.mark.parametrize("failure", ["foreign_face", "duplicate_face", "incomplete_census"])
def test_late_mapping_refusal_retains_exact_completed_result(monkeypatch, failure) -> None:
    prepared = prepare_framed_part(_full_gauge_part())
    assert isinstance(prepared, PreparedFramedPart)
    products = []
    original_inventory = frames._take_inventory

    def counted(*args, **kwargs):
        product = original_inventory(*args, **kwargs)
        products.append(product)
        return product

    monkeypatch.setattr(frames, "_take_inventory", counted)
    if failure == "incomplete_census":
        monkeypatch.setattr(
            evidence_module.RecognitionEvidence, "faces", property(lambda self: frozenset())
        )
    else:
        face = (
            Box(1, 2, 3).faces()[0]
            if failure == "foreign_face"
            else prepared.part.faces()[0]
        )
        monkeypatch.setattr(evidence_module.RecognitionEvidence, "face", lambda self, ref: face)

    refused = prepared.recognise_evidence(rotational=True)

    assert isinstance(refused, RefusedFramedEvidence)
    assert refused.reason is FramedEvidenceRefusalReason.CALLER_FACE_MAPPING_UNAVAILABLE
    assert isinstance(refused.result, FramedRecognitionResult)
    assert len(products) == 1
    assert refused.result.frame is prepared.frame
    assert refused.result.part is prepared.part
    assert refused.result.result is products[0].result
    assert refused.result.result.rotational is True
    for actual, original in zip(refused.result.result.cylinders, prepared.cylinders, strict=True):
        assert len(actual) == len(original)
        assert all(a is b for a, b in zip(actual, original, strict=True))
    # The public carrier cannot leak the private product or a partial evidence authority.
    assert {field.name for field in fields(refused)} == {"reason", "result"}
    with pytest.raises(FrozenInstanceError):
        refused.result = None
    # A caller's ordinary fallback consumes the retained aggregate, without recognizing again.
    fallback = refused.result if refused.result is not None else prepared.recognise()
    assert fallback is refused.result
    assert len(products) == 1


def test_framed_face_references_retain_exact_view_authority() -> None:
    first = build_framed_recognition_evidence(Box(10, 20, 30).rotate(Axis.X, 30))
    second = build_framed_recognition_evidence(Box(10, 20, 30).rotate(Axis.X, 30))
    assert isinstance(first, FramedRecognitionEvidence)
    assert isinstance(second, FramedRecognitionEvidence)
    reference = next(iter(first.faces))

    with pytest.raises(ValueError, match="foreign, copied, forged, or stale"):
        second.caller_face(reference)
    with pytest.raises(TypeError, match="FaceRef"):
        first.caller_face(first.features[0] if first.features else object())  # type: ignore[arg-type]

    copied = object.__new__(FaceRef)
    object.__setattr__(
        copied,
        "_FaceRef__authority",
        object.__getattribute__(reference, "_FaceRef__authority"),
    )
    with pytest.raises(ValueError, match="foreign, copied, forged, or stale"):
        first.caller_face(copied)


def test_framed_builder_preserves_existing_typed_frame_refusal() -> None:
    refusal = build_framed_recognition_evidence(Compound(children=[]))

    assert isinstance(refusal, RefusedPartFrame)
    assert refusal == frames.infer_part_frame(Compound(children=[]))


def test_framed_evidence_cannot_be_constructed_without_issuer() -> None:
    with pytest.raises(TypeError, match="framed evidence lifecycle"):
        FramedRecognitionEvidence()
