"""Schema projections must not look like unrelated detector regressions."""

from dataclasses import replace

import pytest

from tests.test_rectangular_blind_slots import _slot as rectangular_slot
from tests.test_round_bottom_slots import _slot as round_slot
from tools import benchmark_rectangular_blind_slots as rectangular
from tools import benchmark_round_bottom_slots as rounded


@pytest.mark.parametrize(
    "module,fixture,key",
    [
        (rectangular, rectangular_slot, "all_other_outputs_equal"),
        (rounded, round_slot, "all_pre_existing_outputs_equal"),
    ],
)
def test_benchmark_ignores_only_projection_changes(module, fixture, key, monkeypatch):
    part = fixture()
    measured = {enabled: module._run_case(part, enabled) for enabled in (False, True)}
    assert measured[False][0].result.section_recesses != measured[True][0].result.section_recesses
    monkeypatch.setattr(module, "_run_case", lambda _part, enabled: measured[enabled])
    assert module._measure([("authored", part)])[key] is True
    if module is rectangular:
        assert module._measure([("authored", part)])["all_pocket_deltas_explained"] is True

    # A genuinely unrelated detector change must still fail, even after filtering projections.
    product, seconds = measured[True]
    measured[True] = (
        replace(product, _legacy_result=replace(product._legacy_result, slots=(object(),))),
        seconds,
    )
    assert module._measure([("authored", part)])[key] is False
