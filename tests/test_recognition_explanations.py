# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Public bounded explanations over one aggregate recognition run."""

from __future__ import annotations

from dataclasses import FrozenInstanceError, replace

import pytest
from build123d import (
    Box,
    BuildPart,
    BuildSketch,
    Edge,
    Face,
    Plane,
    Polygon,
    Pos,
    Rot,
    Shell,
    Solid,
    Wire,
    extrude,
)

import quiddity as r
import quiddity.evidence as evidence_module
import quiddity.explanations as explanation_module
import quiddity.frames as frame_module
from quiddity._candidates import FamilyId
from quiddity._diagnostics import DiagnosticCode, DiagnosticStatus, ResidualDiagnostic
from quiddity._dispositions import Outcome, ReasonCode
from quiddity._registry import PHYSICAL_DEFINITIONS


def _u_passage():
    with BuildPart() as tool:
        with BuildSketch(Plane.XY):
            Polygon(
                (-15, -15),
                (15, -15),
                (15, 15),
                (9, 15),
                (9, -9),
                (-9, -9),
                (-9, 15),
                (-15, 15),
            )
        extrude(amount=40, both=True)
    return Box(60, 60, 20) - tool.part


def _family(report: r.RecognitionReport, family: str) -> r.FamilyExplanation:
    return next(item for item in report.families if item.family == family)


def _side_subdivided_blind_step() -> Solid:
    part = Box(60, 40, 12) - Pos(-20, 20, 6) * Rot(45, 0, 0) * Box(
        30, 5.657, 5.657
    )
    faces = list(part.faces())
    slant = next(
        face
        for face in faces
        if abs(face.normal_at().Y) > 0.5 and abs(face.normal_at().Z) > 0.5
    )
    terminal = next(
        face
        for face in faces
        if len(face.edges()) == 3 and abs(face.normal_at().X) > 0.9
    )
    common = next(
        first
        for first in terminal.edges()
        for second in slant.edges()
        if first.is_same(second)
    )
    start, end = (vertex.center() for vertex in common.vertices())
    middle = (start + end) * 0.5
    split_edges = [Edge.make_line(start, middle), Edge.make_line(middle, end)]
    new_terminal = Face(
        Wire([edge for edge in terminal.edges() if not edge.is_same(common)] + split_edges)
    )
    new_slant = Face(
        Wire([edge for edge in slant.edges() if not edge.is_same(common)] + split_edges)
    )
    if new_terminal.normal_at().dot(terminal.normal_at()) < 0:
        new_terminal = Face(new_terminal.outer_wire().reversed())
    if new_slant.normal_at().dot(slant.normal_at()) < 0:
        new_slant = Face(new_slant.outer_wire().reversed())
    result = Solid(
        Shell(
            [
                face
                for face in faces
                if not face.is_same(slant) and not face.is_same(terminal)
            ]
            + [new_terminal, new_slant]
        )
    )
    assert result.is_valid
    return result


def test_report_preserves_result_and_closed_family_roster() -> None:
    report = r.build_recognition_report(Box(10, 10, 10))

    assert report.coverage is r.ExplanationCoverage.BOUNDED
    assert report.result == r.build_recognition_result(Box(10, 10, 10))
    assert tuple(item.family for item in report.families) == tuple(
        definition.family.value for definition in PHYSICAL_DEFINITIONS
    )
    assert all(item.evaluation is r.FamilyEvaluation.EVALUATED for item in report.families)
    assert all(
        (item.proposed, item.accepted, item.rejected, item.dispositions) == (0, 0, 0, ())
        for item in report.families
    )
    assert all(item.proposed == item.accepted + item.rejected for item in report.families)
    assert report.diagnostics == ()


def test_legacy_raw_report_name_is_an_exact_compatibility_alias() -> None:
    part = Box(10, 10, 10)

    assert r.build_recognition_report(part) == r.build_raw_recognition_report(part)


def test_reconciliation_loss_is_counted_without_identity_leakage() -> None:
    report = r.build_recognition_report(_u_passage())

    slots = _family(report, "slots")
    assert (slots.proposed, slots.accepted, slots.rejected) == (2, 0, 2)
    assert slots.dispositions == (
        r.DispositionExplanation(
            r.ReconciliationReason.SLOT_SUPERSEDED_BY_PASSAGE,
            r.RecognitionOutcome.REJECTED,
            occurrences=2,
            related_occurrences=2,
        ),
    )
    passage = _family(report, "passages")
    assert (passage.proposed, passage.accepted, passage.rejected) == (1, 1, 0)
    assert passage.dispositions[0].reason is r.ReconciliationReason.DEFAULT_ACCEPTED
    assert report.result.slots == ()
    assert sum(record.classification.feature_kind == "passage"
               for record in report.result.section_recesses) == 1
    assert all(item.proposed == item.accepted + item.rejected for item in report.families)


def test_classification_distinguishes_not_applicable_from_evaluated_empty() -> None:
    report = r.build_recognition_report(Box(10, 10, 10), rotational=True)

    # Plates evaluate per body so a rotational compound cannot hide an independent prismatic
    # member merely because another member established a turned profile.
    assert _family(report, "plates").evaluation is r.FamilyEvaluation.EVALUATED
    assert _family(report, "angled_steps").evaluation is r.FamilyEvaluation.NOT_APPLICABLE
    assert _family(report, "holes").evaluation is r.FamilyEvaluation.EVALUATED
    assert _family(report, "holes").proposed == 0


def test_public_closed_values_match_private_authority() -> None:
    def exact_members(enum_type):
        assert len(enum_type.__members__) == len(enum_type)
        return {name: member.value for name, member in enum_type.__members__.items()}

    assert exact_members(r.ReconciliationReason) == exact_members(ReasonCode)
    assert exact_members(r.RecognitionOutcome) == exact_members(Outcome)
    assert exact_members(r.RecognitionDiagnosticCode) == exact_members(DiagnosticCode)
    assert exact_members(r.RecognitionDiagnosticStatus) == exact_members(DiagnosticStatus)


def test_public_report_executes_the_inventory_once(monkeypatch) -> None:
    products = []
    original = explanation_module._take_inventory

    def counted(*args, **kwargs):
        product = original(*args, **kwargs)
        products.append(product)
        return product

    monkeypatch.setattr(explanation_module, "_take_inventory", counted)

    report = r.build_recognition_report(Box(10, 10, 10))

    assert len(products) == 1
    assert report.result is products[0].result


def test_recognised_split_terminal_tracks_raw_coordinates_without_a_residual() -> None:
    part = _side_subdivided_blind_step()
    original = r.build_recognition_report(part)
    transformed = r.build_recognition_report(Pos(100, -20, 5) * part)

    assert len(original.result.angled_steps) == len(transformed.result.angled_steps) == 1
    assert original.diagnostics == transformed.diagnostics == ()
    step = original.result.angled_steps[0]
    moved = transformed.result.angled_steps[0]
    assert (moved.axis, moved.leg1, moved.leg2, moved.angle, moved.length) == (
        step.axis,
        step.leg1,
        step.leg2,
        step.angle,
        step.length,
    )
    assert moved.at == (step.at[0] + 100, step.at[1] - 20, step.at[2] + 5)
    assert tuple(
        (item.family, item.evaluation, item.proposed, item.accepted, item.rejected)
        for item in transformed.families
    ) == tuple(
        (item.family, item.evaluation, item.proposed, item.accepted, item.rejected)
        for item in original.families
    )


def test_explanation_values_do_not_expose_private_identity() -> None:
    report = r.build_recognition_report(_u_passage())
    public_values = (*report.families, *report.diagnostics)

    assert all("candidate" not in field for value in public_values for field in value.__slots__)
    assert all("evidence" not in field for value in public_values for field in value.__slots__)
    assert all("node" not in field for value in public_values for field in value.__slots__)


def test_family_lookup_uses_stable_public_family_ids() -> None:
    report = r.build_recognition_report(_u_passage())
    assert {item.family for item in report.families} == {
        family.value for family in FamilyId if family is not FamilyId.LEGACY
    }


@pytest.mark.parametrize("route", ["raw", "framed", "prepared"])
@pytest.mark.parametrize("case", ["reconciliation", "gating", "injected_diagnostic"])
def test_evidence_and_report_share_one_inventory(monkeypatch, route, case) -> None:
    part = {
        "reconciliation": _u_passage,
        "gating": lambda: Box(10, 20, 30),
        "injected_diagnostic": lambda: Box(10, 20, 30),
    }[case]()
    rotational = case == "gating"
    prepared = r.prepare_framed_part(part)
    assert isinstance(prepared, r.PreparedFramedPart)
    expected = (
        r.build_raw_recognition_report(part, rotational=rotational)
        if route == "raw"
        else prepared.recognise_report(rotational=rotational).report
    )
    # Inject a bounded diagnostic at the inventory boundary to test publication independently
    # of whether a particular authored shape still triggers this residual as detectors improve.
    diagnostic = ResidualDiagnostic(
        DiagnosticCode.UNSUPPORTED_SUBDIVIDED_ANGLED_STEP_TERMINAL,
        DiagnosticStatus.UNSUPPORTED, "angled_steps", "x", (1.0, 2.0, 3.0), 4, 3,
    )
    if case == "injected_diagnostic":
        expected = replace(expected, diagnostics=(r.RecognitionDiagnostic(
            r.RecognitionDiagnosticCode.UNSUPPORTED_SUBDIVIDED_ANGLED_STEP_TERMINAL,
            r.RecognitionDiagnosticStatus.UNSUPPORTED, "angled_steps", "x",
            (1.0, 2.0, 3.0), 4, 3,
        ),))
    original_inventory = evidence_module._take_inventory
    products = []

    def counted(*args, **kwargs):
        product = original_inventory(*args, **kwargs)
        if case == "injected_diagnostic":
            product = replace(product, diagnostics=(diagnostic,))
        products.append(product)
        return product

    for module in (evidence_module, frame_module, explanation_module):
        monkeypatch.setattr(module, "_take_inventory", counted)
    if route == "raw":
        view = evidence_module.build_recognition_evidence(part, rotational=rotational)
    elif route == "framed":
        view = r.build_framed_recognition_evidence(part, rotational=rotational)
    else:
        view = prepared.recognise_evidence(rotational=rotational)

    assert isinstance(view, (evidence_module.RecognitionEvidence, r.FramedRecognitionEvidence))
    assert len(products) == 1
    assert view.report is view.report
    assert view.report.result is view.result is products[0].result
    assert view.report == expected
    assert view.report.coverage is r.ExplanationCoverage.BOUNDED
    if case == "reconciliation":
        assert _family(view.report, "slots").rejected == 2
    elif case == "gating":
        assert _family(view.report, "angled_steps").evaluation is r.FamilyEvaluation.NOT_APPLICABLE
    else:
        assert view.report.diagnostics
    with pytest.raises(AttributeError):
        view.report = expected
    with pytest.raises(FrozenInstanceError):
        view.report.result = None
    assert len(products) == 1
