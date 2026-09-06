"""Authored occurrence-local publication failure regressions (#547)."""

import pytest
from build123d import Compound, Pos

import quiddity.result as result_module
from quiddity._section_adapters import LegacySectionProjectionError
from tests.test_rectangular_blind_slots import _slot


@pytest.mark.parametrize("error_type", [ValueError, LegacySectionProjectionError])
def test_failed_open_projection_preserves_other_body_and_reports_evidence(monkeypatch, error_type):
    part = Compound([_slot(), Pos(100, 0, 0) * _slot()])
    baseline = result_module.build_recognition_result(part)
    assert len(baseline.section_recesses) == 2
    original = result_module._principal_open_geometry
    calls = 0

    def fail_first(**kwargs):
        nonlocal calls
        calls += 1
        if calls == 1:
            raise error_type("authored publication failure")
        return original(**kwargs)

    monkeypatch.setattr(result_module, "_principal_open_geometry", fail_first)
    result = result_module.build_recognition_result(part)
    assert calls == 2
    assert len(result.section_recesses) == 1
    (refusal,) = result.section_recess_refusals
    assert refusal.evidence.defining_faces
    assert refusal.evidence.constituent_faces
    assert refusal.body != result.section_recesses[0].body
    assert result.section_recesses[0].index == 0
    assert result.section_recesses[0].geometry == baseline.section_recesses[1].geometry


@pytest.mark.parametrize("error_type", [ValueError, RuntimeError, TypeError])
def test_projection_loop_does_not_hide_proof_or_ownership_errors(error_type):
    def broken_proof(*args, **kwargs):
        raise error_type("source invariant failed")

    with pytest.raises(error_type, match="source invariant failed"):
        result_module._project_recess_records([object()], broken_proof, context=None, evidence=None)


@pytest.mark.parametrize("error_type", [RuntimeError, TypeError])
def test_publication_wrapper_does_not_hide_unexpected_errors(error_type):
    def broken_constructor():
        raise error_type("unexpected bug")

    with pytest.raises(error_type, match="unexpected bug"):
        result_module._publication_value(broken_constructor)
