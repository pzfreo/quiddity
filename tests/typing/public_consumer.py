"""Strict external-consumer fixture, checked against the built wheel."""

from typing import Literal

from build123d import BoundBox, Compound, Edge, Face, Shape, Solid
from OCP.TopoDS import TopoDS_Shape
from typing_extensions import assert_type

from quiddity import (
    BossRecord,
    FramedEvidence,
    FramedEvidenceRefusalReason,
    FramedRecognitionEvidence,
    FramedRecognitionReport,
    FramedRecognitionResult,
    HoleRecord,
    PairedRampStep,
    PartFrame,
    Plate,
    PreparedFramedPart,
    RaisedPad,
    RecognitionReport,
    RecognitionResult,
    RefusedFramedEvidence,
    SectionRecess,
    SectionRecessArray,
    SectionRecessDocument,
    SectionRecessGrid,
    SectionRecessRefusal,
    build_framed_recognition_evidence,
    build_framed_recognition_report,
    build_framed_recognition_result,
    build_raw_recognition_report,
    build_raw_recognition_result,
    build_recognition_report,
    build_recognition_result,
    build_section_recess_document,
    classify_bevel,
    feature_census,
    prepare_framed_part,
    recognise_bosses,
    recognise_holes,
    recognise_paired_ramp_steps,
    recognise_plates,
)
from quiddity.evidence import (
    AssociationMeasure,
    FaceRef,
    FamilyAssociation,
    FeatureRef,
    GeometryAssociation,
    RecognitionEvidence,
    RecognitionRecord,
    build_recognition_evidence,
)
from quiddity.inspection import (
    BevelReject,
    FaceInspection,
    cone_rims,
    floor_face_anchor,
    inspect_face,
    read_double_d_tool,
)


def consume_bevel_rejection(error: BevelReject) -> None:
    assert_type(
        error.reason,
        Literal["nonplanar", "degenerate", "aligned", "compound"],
    )


def consume(part: Solid, face: Face, bounds: BoundBox) -> None:
    document = build_section_recess_document(part)
    assert_type(document, SectionRecessDocument)
    assert_type(document.occurrences, tuple[SectionRecess, ...])
    assert_type(document.refusals, tuple[SectionRecessRefusal, ...])
    assert_type(document.patterns, tuple[SectionRecessArray | SectionRecessGrid, ...])
    holes = recognise_holes(part)
    bosses = recognise_bosses(part)
    paired_ramp_steps = recognise_paired_ramp_steps(part)
    plates = recognise_plates(part)
    result = build_recognition_result(part)
    report = build_recognition_report(part)
    raw_result = build_raw_recognition_result(part)
    raw_report = build_raw_recognition_report(part)
    evidence = build_recognition_evidence(part)
    framed_evidence: FramedEvidence = build_framed_recognition_evidence(part)

    assert_type(holes, list[HoleRecord])
    assert_type(bosses, list[BossRecord])
    assert_type(paired_ramp_steps, list[PairedRampStep])
    assert_type(plates, list[Plate])
    assert_type(result, RecognitionResult)
    assert_type(report, RecognitionReport)
    assert_type(report.result, RecognitionResult)
    assert_type(raw_result, RecognitionResult)
    assert_type(raw_report, RecognitionReport)
    assert_type(evidence, RecognitionEvidence)
    assert_type(evidence.result, RecognitionResult)
    assert_type(evidence.features, tuple[FeatureRef, ...])
    assert_type(evidence.faces, frozenset[FaceRef])
    assert_type(evidence.association, GeometryAssociation)
    assert_type(evidence.association.face_count, AssociationMeasure[int])
    assert_type(evidence.association.surface_area, AssociationMeasure[float])
    assert_type(evidence.association.face_count.ratio, float | None)
    assert_type(evidence.association.families, tuple[FamilyAssociation, ...])
    assert_type(evidence.association.unassociated_faces, frozenset[FaceRef])
    for feature in evidence.features:
        assert_type(evidence.family(feature), str)
        assert_type(evidence.record(feature), RecognitionRecord)
        assert_type(evidence.defining_faces(feature), frozenset[FaceRef])
        assert_type(evidence.constituent_faces(feature), frozenset[FaceRef])
    for reference in evidence.faces:
        assert_type(evidence.face(reference), Face)
    if isinstance(framed_evidence, FramedRecognitionEvidence):
        assert_type(framed_evidence.frame, PartFrame)
        assert_type(framed_evidence.part, Shape[TopoDS_Shape])
        assert_type(framed_evidence.caller_part, Solid | Compound)
        assert_type(framed_evidence.result, RecognitionResult)
        assert_type(framed_evidence.features, tuple[FeatureRef, ...])
        for reference in framed_evidence.faces:
            assert_type(framed_evidence.face(reference), Face)
            assert_type(framed_evidence.caller_face(reference), Face)
    if isinstance(framed_evidence, RefusedFramedEvidence):
        assert_type(framed_evidence.reason, FramedEvidenceRefusalReason)
        assert_type(framed_evidence.result, FramedRecognitionResult | None)
    assert_type(result.holes, tuple[HoleRecord, ...])
    assert_type(result.bosses, tuple[BossRecord, ...])
    assert_type(result.paired_ramp_steps, tuple[PairedRampStep, ...])
    assert_type(result.plates, tuple[Plate, ...])
    assert_type(result.pads, tuple[RaisedPad, ...])
    for pad in result.pads:
        assert_type(pad.axis, str)
        assert_type(pad.direction, int)
    framed = build_framed_recognition_result(part)
    if isinstance(framed, FramedRecognitionResult):
        assert_type(framed.part, Shape[TopoDS_Shape])
    framed_report = build_framed_recognition_report(part)
    if isinstance(framed_report, FramedRecognitionReport):
        assert_type(framed_report.part, Shape[TopoDS_Shape])
        assert_type(framed_report.report, RecognitionReport)
    prepared = prepare_framed_part(part)
    if isinstance(prepared, PreparedFramedPart):
        assert_type(prepared.part, Shape[TopoDS_Shape])
        assert_type(prepared.recognise(rotational=True), FramedRecognitionResult)
        assert_type(prepared.recognise_report(rotational=True), FramedRecognitionReport)
        prepared_evidence = prepared.recognise_evidence(rotational=True)
        if isinstance(prepared_evidence, FramedRecognitionEvidence):
            assert_type(prepared_evidence.result, RecognitionResult)
    assert_type(result.step_ladder_for_z_span(0.0, 10.0), list[float])
    assert_type(result.step_ladder(bounds), list[float])
    assert_type(feature_census(part), dict[str, int])
    assert_type(
        classify_bevel(face),
        tuple[
            int,
            tuple[float, float, float],
            dict[int, tuple[float, float]],
            float,
            float,
        ],
    )
    assert_type(inspect_face(face), FaceInspection)
    assert_type(cone_rims(face), tuple[Edge, Edge, float] | None)
    assert_type(floor_face_anchor(face), tuple[float, float, float])
    assert_type(
        read_double_d_tool(part),
        tuple[str, float, float, tuple[float, float, float], float, tuple[float, float, float]],
    )
