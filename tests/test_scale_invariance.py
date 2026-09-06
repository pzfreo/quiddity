# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

"""Recognition of the golden corpus rebuilt at other scales.

A recogniser that compares two measurements of one feature against an absolute millimetre band
answers differently for the same feature modelled in metres, inches or on a micro-part. ADR 0008
makes those *tolerances* proportional, and this test pins the families that depend only on them,
so a later absolute constant cannot quietly reintroduce the fault.

Coverage is partial and deliberately so. Families gated by a *minimum-evidence threshold* —
"is this big enough to be a feature?" — are excluded, because 0.2.4 makes those thresholds
absolute again after 0.2.3 scaled them and erased small features on large parts (issue #72).

Whether a very small geometric feature is worth reporting is consumer policy under ADR 0001.
Chamfers therefore have no default size floor and remain in the scale-free census; callers may
still pass an explicit ``tol`` when they intentionally want a reporting threshold.
"""

from __future__ import annotations

import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).parents[1]
GOLDEN_ROOT = ROOT / "tests" / "golden"
sys.path.insert(0, str(ROOT))

from build123d import Box, Pos, import_step  # noqa: E402

from quiddity import (  # noqa: E402
    recognise_risers,
    recognise_turned_steps,
    step_level_zs,
)
from quiddity._geometry import clears_threshold, quantise  # noqa: E402
from tests.golden._common import load_fixture  # noqa: E402
from tools._legacy_recognition import feature_census  # noqa: E402

CASES = sorted(GOLDEN_ROOT.glob("*/fixture.py"))

#: Both extremes plus one interior factor. The interior one earns its place: ``plates`` used to
#: gain a spurious record in ``chamfers_fillets_and_flats`` at 5x and 10x but not at 1x or 100x,
#: which a test visiting only the extremes would have missed entirely.
FACTORS = (0.05, 5.0, 100.0)

#: Kinds whose recogniser applies an absolute *minimum-evidence threshold*, so shrinking a part
#: far enough legitimately takes the feature below it. These are excluded by design, not pending
#: conversion — see ADR 0008 on why a threshold must not scale with the part.
# Aggregate Blend membership is also threshold-dependent: direct Blend geometry is scale-free,
# but an accepted thresholded Fillet deliberately supersedes the same complete chain. Its direct
# scale contract is pinned in ``tests/test_blends.py`` rather than erased from that ownership rule.
NOT_YET_SCALE_FREE = frozenset({"blend", "fillet", "flat", "plate", "pocket", "channel"})

# This test partitions internal detectors by their admission thresholds. A unified public
# section_recess can come from either partition and must not erase that distinction.


def _scale_free_census(part) -> dict[str, int]:
    return {
        kind: count
        for kind, count in feature_census(part).items()
        if count and kind not in NOT_YET_SCALE_FREE
    }


@pytest.mark.parametrize("factor", FACTORS)
@pytest.mark.parametrize("fixture_path", CASES, ids=lambda path: path.parent.name)
def test_the_turned_profile_measures_the_same_shaft_at_any_scale(fixture_path, factor):
    """Not the count of rungs -- what each rung says the diameter is.

    The census check below compares how many features there are. That is not enough, and this
    test exists because it was not: `recognise_turned_steps` reported *every* rung of the
    pinned turned-step-and-groove shaft at the shaft's OD when the part was modelled at 0.05x,
    describing a plain shaft where there is a groove. Three rungs at 1x and three rungs at
    0.05x agree, so counting saw nothing while every diameter was wrong.

    Diameters are compared as ratios to the largest, which is scale-free by construction and
    needs no tolerance on a length.
    """

    fixture = load_fixture(fixture_path)

    def profile(part) -> list[tuple[float, float]]:
        steps = recognise_turned_steps(part)
        if not steps:
            return []
        widest = max(step.diameter for step in steps)
        length = max(step.hi for step in steps) - min(step.lo for step in steps)
        return [
            (round(step.diameter / widest, 6), round((step.hi - step.lo) / length, 6))
            for step in sorted(steps, key=lambda step: step.lo)
        ]

    expected = profile(fixture.build_fixture())
    actual = profile(fixture.build_fixture().scale(factor))

    assert actual == expected, f"{fixture_path.parent.name} at {factor}x"


@pytest.mark.parametrize("factor", FACTORS)
@pytest.mark.parametrize("fixture_path", CASES, ids=lambda path: path.parent.name)
def test_converted_families_recognise_the_same_features_at_any_scale(fixture_path, factor):
    """The same solid, modelled *factor* times larger, is the same set of features."""

    fixture = load_fixture(fixture_path)
    expected = _scale_free_census(fixture.build_fixture())

    actual = _scale_free_census(fixture.build_fixture().scale(factor))

    assert actual == expected, f"{fixture_path.parent.name} at {factor}x"


def test_a_magnitude_exactly_on_its_threshold_is_decided_the_same_way_at_every_scale():
    """An area gate compared at exact equality is settled by rounding, not by the part.

    ``chamfers_fillets_and_flats`` has a face whose area is exactly ``min_area_frac`` of the
    cross-section. ``area - threshold`` came out ``-1.7e-13`` at 1x, exactly ``0.0`` at 5x and
    10x, and ``-9.3e-10`` at 100x, so ``recognise_plates`` returned a record at two scales and
    not the others — the same geometry classified two ways.
    """

    assert not clears_threshold(480.0, 480.0), "an exact tie must not admit a feature"
    assert not clears_threshold(479.9999999999999, 480.00000000000006)
    assert clears_threshold(480.1, 480.0), "a real margin must still clear it"

    fixture = load_fixture(GOLDEN_ROOT / "chamfers_fillets_and_flats" / "fixture.py")
    counts = {
        factor: feature_census(fixture.build_fixture().scale(factor)).get("plate", 0)
        for factor in (0.2, 5.0, 10.0, 100.0)
    }

    assert set(counts.values()) == {0}, counts


def test_an_explicit_tolerance_keeps_its_literal_millimetre_meaning():
    """``tol=None`` resolves from the part; a float passed in must not be re-scaled.

    This is the compatibility half of ADR 0008 and the reason the change is safe for a caller
    who has already calibrated against their own geometry. ``RiserEvidence.tol`` reports what
    the scan actually used, so it can be read back directly rather than inferred from counts.
    """

    part = Box(60, 60, 10) + Pos(0, 0, 7.5) * Box(30, 60, 5)

    explicit = {riser.tol for riser in recognise_risers(part, tol=0.25)}

    assert explicit == {0.25}
    assert {riser.tol for riser in recognise_risers(part.scale(10), tol=0.25)} == {0.25}

    # The default no longer follows the part, which is the 0.2.4 correction: a riser's existence
    # must not depend on how large the plate around it is.
    assert {riser.tol for riser in recognise_risers(part)} == {
        riser.tol for riser in recognise_risers(part.scale(10))
    }


def test_an_explicit_end_margin_is_also_honoured_literally():
    """The step-ladder inset takes the same ``None``-resolves/float-is-literal contract."""

    part = Box(40, 40, 20) + Pos(0, 0, 12.5) * Box(20, 40, 5)

    assert step_level_zs(part, tol=0.1) == step_level_zs(part, tol=0.1)
    assert step_level_zs(part) == step_level_zs(part)


def test_the_precision_floor_follows_the_value_and_not_the_millimetre():
    """`quantise` is the scale-free `round`, and the reason the substrate needed one.

    A decimal quantum is a length: `round(x, 2)` is 0.16% of a 6.25 mm band and 8% of the same
    band on a part modelled at a twentieth, which is how `analyse_cylinders` came to report a
    Ø6.25 band as Ø0.31. Significant figures keep the quantum proportional, and — unlike a
    relative *tolerance* — still form a grid, so a quantised value can be a grouping key.
    """

    assert quantise(6.25) == 6.25
    assert quantise(0.3125) == 0.3125, "the case the decimal quantum lost"
    assert quantise(6.000000000000001) == 6.0, "kernel noise still goes"

    # The guarantee is a relative error bound at any scale — checked over a spread of values
    # rather than a few, because the first version of this test asserted commutativity with
    # scaling on three hand-picked values and that property is false for about a fifth of them.
    # Derived, not picked: rounding to N significant figures moves a value by at most half a
    # unit in the Nth digit, and the mantissa is at least 1, so the relative error is bounded by
    # 0.5 * 10**(1-N). Writing 1e-6 here instead failed at a mantissa near 1.0, which is exactly
    # where the bound is tightest.
    figures = 6
    bound = 0.5 * 10 ** (1 - figures)
    step = 0.0007
    value = 0.001
    while value < 1000:
        for factor in (1.0, 0.05, 5.0, 100.0):
            scaled = value * factor
            assert abs(quantise(scaled) - scaled) <= bound * abs(scaled), (value, factor)
        value += step
        step *= 1.05

    # Still a grid, so it can key a dict — two values a millionth apart land together.
    assert quantise(6.25, figures=4) == quantise(6.2500004, figures=4)


TURNED_CASES = sorted((ROOT / "tests" / "corpus" / "gramel").glob("*.step"))


@pytest.mark.parametrize("factor", FACTORS)
@pytest.mark.parametrize("path", TURNED_CASES, ids=lambda path: path.stem)
def test_the_census_of_a_real_turned_part_is_the_same_at_any_scale(path, factor):
    """The check the golden fixtures cannot make, on parts that were actually turned.

    Every scale test above runs on the golden corpus, and the two vendored third-party corpora
    are milled prismatic — measured, all 50 NIST and MFCAD++ models report zero turned steps.
    So the families whose gates this suite exists to police had, until these three parts, no
    real geometry checking them at any scale but 1×.

    That gap is not hypothetical. It is where the rung that reported its neighbour's diameter
    lived, and the shoulder that split a rung in two, and the diameter quantised to a hundredth
    of a millimetre — three defects, none of which the golden corpus could see.
    """

    part = import_step(str(path))

    assert _scale_free_census(part.scale(factor)) == _scale_free_census(part)
