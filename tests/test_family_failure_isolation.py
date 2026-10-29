"""Synthetic geometry and injected failures for the opt-in contract proposed in #806."""

from concurrent.futures import CancelledError
from dataclasses import FrozenInstanceError, asdict, replace

import pytest
from build123d import Align, Box, Cylinder, Pos

import quiddity as q
import quiddity.frames as frames
import quiddity.result as orchestration
from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId, PredicateId, SplitTriangularTerminalFact
from quiddity._claims import ClaimLedger
from quiddity.evidence import RecognitionEvidence, build_recognition_evidence


def annular_pocket():
    """The valid, 16-face box construction from #805; no customer geometry."""
    align = (Align.MIN,) * 3
    stock = Box(40, 30, 8, align=align)
    pocket = Pos(5, 5, 4) * Box(30, 20, 4, align=align)
    island = Pos(15, 11, 4) * Box(10, 8, 4, align=align)
    return stock - (pocket - island)


def pad_and_hole():
    return Box(40, 30, 8) + Pos(10, 0, 6) * Box(8, 8, 4) - Pos(-10, 0, 0) * Cylinder(2, 20)


def build_view(route, part, **kwargs):
    if route == "raw-report":
        return q.build_raw_recognition_report(part, **kwargs)
    if route == "report-alias":
        return q.build_recognition_report(part, **kwargs)
    if route == "raw-evidence":
        return build_recognition_evidence(part, **kwargs)
    if route == "framed-report":
        return q.build_framed_recognition_report(part, **kwargs)
    if route == "framed-evidence":
        return q.build_framed_recognition_evidence(part, local_degradation=True, **kwargs)
    prepared = q.prepare_framed_part(part)
    assert isinstance(prepared, q.PreparedFramedPart)
    if route == "prepared-report":
        return prepared.recognise_report(**kwargs)
    return prepared.recognise_evidence(**kwargs)


ROUTES = (
    "raw-report",
    "report-alias",
    "raw-evidence",
    "framed-report",
    "framed-evidence",
    "prepared-report",
    "prepared-evidence",
)


@pytest.mark.parametrize("route", ROUTES)
def test_real_pocket_failure_keeps_independent_pad_and_exact_evidence(route):
    part = annular_pocket()
    assert part.is_valid and len(part.solids()) == 1 and len(part.faces()) == 16
    assert len(q.recognise_rectangular_pads(part)) == 1
    with pytest.raises(StopIteration):
        build_view(route, part)
    view = build_view(route, part, continue_on_error=True)
    report = view if isinstance(view, q.RecognitionReport) else view.report
    assert len(report.result.pads) == 1
    assert not report.execution_complete
    failure = next(item for item in report.failures if item.family == "prismatic_pockets")
    assert asdict(failure) == {
        "family": "prismatic_pockets",
        "stage": "discovery",
        "exception_type": "builtins.StopIteration",
        "message": "",
        "blocked_by": (),
    }
    families = {item.family: item for item in report.families}
    assert families["prismatic_pockets"].evaluation is q.FamilyEvaluation.FAILED
    assert families["prismatic_pockets"].proposed == 0
    assert families["pads"].evaluation is q.FamilyEvaluation.EVALUATED
    if "evidence" in route:
        pad = next(ref for ref in view.features if view.family(ref) == "pads")
        faces = view.constituent_faces(pad)
        assert len(faces) == 5
        assert view.defining_faces(pad) <= faces
        resolve = view.caller_face if isinstance(view, q.FramedRecognitionEvidence) else view.face
        assert all(
            any(resolve(ref).wrapped.IsSame(face.wrapped) for face in part.faces()) for ref in faces
        )


def inject_discover(monkeypatch, family, replacement):
    definitions = orchestration.PHYSICAL_DEFINITIONS
    original = next(item.discover for item in definitions if item.family is family)
    monkeypatch.setattr(
        orchestration,
        "PHYSICAL_DEFINITIONS",
        tuple(
            replace(item, discover=replacement) if item.family is family else item
            for item in definitions
        ),
    )
    return original


def test_failure_after_issuing_candidates_removes_them_and_keeps_other_families(monkeypatch):
    calls = []

    def fail_after_proposals(services, inputs):
        records = original(services, inputs)
        assert len(records) == 1
        calls.append(records[0])
        raise RuntimeError("injected after hole issuance")

    original = inject_discover(monkeypatch, FamilyId.HOLES, fail_after_proposals)
    view = build_recognition_evidence(pad_and_hole(), continue_on_error=True)
    assert len(calls) == 1  # No rerun of earlier families to reconstruct the ledger.
    assert view.result.holes == () and len(view.result.pads) == 1
    assert all(view.family(ref) != "holes" for ref in view.features)
    assert all(item.family != "holes" for item in view.association.families)
    failures = {item.family: item for item in view.report.failures}
    assert failures["holes"].message == "injected after hole issuance"
    assert failures["hole_patterns"].blocked_by == ("holes",)


def test_failed_dependencies_are_not_read_as_evaluated_empty(monkeypatch):
    def fail(*_args):
        raise ValueError("injected countersink failure")

    def must_not_run(*_args):
        pytest.fail("the dependent hole recogniser must not be invoked")

    inject_discover(monkeypatch, FamilyId.COUNTERSINKS, fail)
    inject_discover(monkeypatch, FamilyId.HOLES, must_not_run)
    report = q.build_recognition_report(pad_and_hole(), continue_on_error=True)
    failures = {item.family: item for item in report.failures}
    assert failures["holes"].blocked_by == ("countersinks",)
    assert failures["holes"].exception_type is None
    assert failures["hole_patterns"].blocked_by == ("holes",)
    assert len(report.result.pads) == 1
    families = {item.family: item for item in report.families}
    assert families["holes"].evaluation is q.FamilyEvaluation.DEPENDENCY_FAILED
    assert families["pads"].evaluation is q.FamilyEvaluation.EVALUATED


def test_derived_failure_keeps_its_members(monkeypatch):
    def fail(*_args):
        raise RuntimeError("injected pattern failure")

    monkeypatch.setattr(
        orchestration,
        "DERIVED_DEFINITIONS",
        tuple(
            replace(item, derive=fail) if item.result_field == "hole_patterns" else item
            for item in orchestration.DERIVED_DEFINITIONS
        ),
    )
    part = Box(20, 20, 10) - Cylinder(2, 20)
    with pytest.raises(RuntimeError, match="injected pattern"):
        q.build_recognition_report(part)
    report = q.build_recognition_report(part, continue_on_error=True)
    assert len(report.result.holes) == 1 and report.result.hole_patterns == ()
    assert len(report.failures) == 1
    assert report.failures[0].family == "hole_patterns"
    assert report.failures[0].stage == "derivation"


@pytest.mark.parametrize(
    "error",
    [
        MemoryError("memory"),
        OSError("resource"),
        RecursionError("stack"),
        SystemError("interpreter"),
        CancelledError(),
        KeyboardInterrupt(),
        SystemExit(),
        ExceptionGroup("group", [ValueError("local"), MemoryError("memory")]),
    ],
)
def test_resource_and_control_failures_still_propagate(monkeypatch, error):
    def fail(*_args):
        raise error

    inject_discover(monkeypatch, FamilyId.COUNTERSINKS, fail)
    with pytest.raises(type(error)) as caught:
        q.build_recognition_report(Box(10, 10, 10), continue_on_error=True)
    assert caught.value is error


def test_wrapped_resource_failure_is_not_swallowed(monkeypatch):
    def fail(*_args):
        try:
            raise MemoryError("memory")
        except MemoryError as cause:
            raise RuntimeError("wrapper") from cause

    inject_discover(monkeypatch, FamilyId.COUNTERSINKS, fail)
    with pytest.raises(RuntimeError, match="wrapper"):
        q.build_recognition_report(Box(10, 10, 10), continue_on_error=True)


def test_output_contract_errors_are_fatal(monkeypatch):
    inject_discover(monkeypatch, FamilyId.COUNTERSINKS, lambda *_args: [object()])
    with pytest.raises(TypeError):
        q.build_recognition_report(Box(10, 10, 10), continue_on_error=True)


@pytest.mark.parametrize("phase", ["start", "_reconcile_existing", "_project_result"])
def test_shared_phases_are_outside_recovery(monkeypatch, phase):
    def fail(*_args, **_kwargs):
        raise RuntimeError("shared failure")

    monkeypatch.setattr(orchestration, phase, fail)
    with pytest.raises(RuntimeError, match="shared failure"):
        q.build_recognition_report(Box(10, 10, 10), continue_on_error=True)


def test_discard_invalidates_candidates_claims_memberships_and_observations():
    graph = FaceGraph(Box(10, 10, 10))
    ledger = ClaimLedger(graph)
    first, second = graph.nodes[:2]
    kept = ledger.sink.propose(FamilyId.FLATS, object(), defining=(first,))
    ledger.candidate_set_for(FamilyId.FLATS, [kept.record])
    discarded = ledger.sink.propose(
        FamilyId.ANGLED_STEPS, object(), defining=(second,), constituent=(first, second)
    )
    ledger.sink.observe(
        FamilyId.ANGLED_STEPS,
        PredicateId.ANGLED_STEP_TERMINAL,
        subject=second,
        consulted=(first,),
        fact=SplitTriangularTerminalFact(4),
    )
    ledger.discard_family(FamilyId.ANGLED_STEPS)
    ledger.candidate_set_for(FamilyId.ANGLED_STEPS, [])
    index = ledger.freeze_index()
    assert len(ledger) == 1 and ledger.claims[0].claimant is kept.record
    assert index.claims_of(first) == (kept,)
    assert index.claims_of(second) == ()
    assert index.memberships_of(first) == (kept,)
    assert index.memberships_of(second) == ()
    assert index.observations(FamilyId.ANGLED_STEPS, PredicateId.ANGLED_STEP_TERMINAL) == ()
    with pytest.raises(ValueError, match="not present"):
        index.defining_of(discarded)
    with pytest.raises(RuntimeError):
        ledger.discard_family(FamilyId.FLATS)


def test_successful_report_is_identical_with_and_without_opt_in():
    part = pad_and_hole()
    strict = q.build_recognition_report(part)
    tolerant = q.build_recognition_report(part, continue_on_error=True)
    assert strict == tolerant
    assert tolerant.execution_complete and tolerant.failures == ()


def test_gated_family_stays_not_applicable_even_if_a_dependency_failed(monkeypatch):
    def fail(*_args):
        raise RuntimeError("injected")

    inject_discover(monkeypatch, FamilyId.COUNTERSINKS, fail)
    monkeypatch.setattr(
        orchestration,
        "PHYSICAL_DEFINITIONS",
        tuple(
            replace(item, dependencies=(FamilyId.COUNTERSINKS,))
            if item.family is FamilyId.ANGLED_STEPS
            else item
            for item in orchestration.PHYSICAL_DEFINITIONS
        ),
    )
    report = q.build_recognition_report(Box(10, 10, 10), rotational=True, continue_on_error=True)
    assert next(f for f in report.families if f.family == "angled_steps").evaluation is (
        q.FamilyEvaluation.NOT_APPLICABLE
    )
    assert all(f.family != "angled_steps" for f in report.failures)


def test_failure_values_are_immutable_and_do_not_retain_the_exception(monkeypatch):
    error = RuntimeError("injected")

    def fail(*_args):
        raise error

    inject_discover(monkeypatch, FamilyId.COUNTERSINKS, fail)
    report = q.build_recognition_report(Box(10, 10, 10), continue_on_error=True)
    failure = report.failures[0]
    assert isinstance(failure, q.RecognitionFailure)
    assert failure.exception_type == "builtins.RuntimeError"
    assert all(not isinstance(value, BaseException) for value in asdict(failure).values())
    with pytest.raises(FrozenInstanceError):
        failure.message = "changed"


@pytest.mark.parametrize("phase", ["validate_output", "_validate_attribution"])
def test_terminal_contract_validation_is_not_recovered(monkeypatch, phase):
    def fail(*_args):
        raise ValueError("invalid evidence contract")

    monkeypatch.setattr(orchestration, phase, fail)
    with pytest.raises(ValueError, match="invalid evidence contract"):
        q.build_recognition_report(Box(10, 10, 10), continue_on_error=True)


def test_completed_evidence_cannot_be_discarded_before_sealing():
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    ledger.candidate_set_for(FamilyId.FLATS, [])
    with pytest.raises(RuntimeError, match="completed"):
        ledger.discard_family(FamilyId.FLATS)


def test_late_mapping_refusal_does_not_leak_an_unlabelled_partial_result(monkeypatch):
    monkeypatch.setattr(RecognitionEvidence, "faces", property(lambda self: frozenset()))
    refused = q.build_framed_recognition_evidence(annular_pocket(), continue_on_error=True)
    assert isinstance(refused, q.RefusedFramedEvidence)
    assert refused.result is None


def test_mapping_refusal_before_inventory_does_not_run_detectors(monkeypatch):
    def must_not_run(*_args, **_kwargs):
        pytest.fail("mapping refusal must precede recognition")

    monkeypatch.setattr(frames, "_caller_face_bijection", lambda *_args: None)
    monkeypatch.setattr(frames, "_take_inventory", must_not_run)
    refused = q.build_framed_recognition_evidence(annular_pocket(), continue_on_error=True)
    assert isinstance(refused, q.RefusedFramedEvidence) and refused.result is None


def test_existing_ownership_error_on_a_valid_solid_still_raises(monkeypatch):
    message = "Hole cylindrical evidence does not prove one valid solid"

    def fail(*_args):
        raise ValueError(message)

    inject_discover(monkeypatch, FamilyId.HOLES, fail)
    with pytest.raises(ValueError, match=message):
        q.build_recognition_report(Box(10, 10, 10), continue_on_error=True)


def test_foreign_evidence_issuance_is_a_fatal_contract_error(monkeypatch):
    foreign = FaceGraph(Box(1, 2, 3)).nodes[0]

    def fail(services, _inputs):
        services.writer.sink.propose(FamilyId.COUNTERSINKS, object(), defining=(foreign,))

    inject_discover(monkeypatch, FamilyId.COUNTERSINKS, fail)
    with pytest.raises(ValueError, match="not this graph"):
        q.build_recognition_report(Box(10, 10, 10), continue_on_error=True)


def test_undeclared_dependency_read_is_a_fatal_contract_error(monkeypatch):
    def fail(_services, inputs):
        inputs.records(FamilyId.PADS, q.RaisedPad)
        return []

    inject_discover(monkeypatch, FamilyId.COUNTERSINKS, fail)
    with pytest.raises(ValueError):
        q.build_recognition_report(Box(10, 10, 10), continue_on_error=True)
