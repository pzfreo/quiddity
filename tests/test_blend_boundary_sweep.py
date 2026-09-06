"""Pinned authored boundary-blend evidence for Epic #290 / issue #277."""

from __future__ import annotations

import json

import pytest

from tools.blend_boundary_sweep import (
    BASELINE_COMMIT,
    IMPLEMENTATION_COMMIT,
    JSON_REPORT,
    MARKDOWN_REPORT,
    PERFORMANCE_BUDGET_SECONDS,
    PERFORMANCE_MEASUREMENT,
    markdown,
    sweep,
)


@pytest.fixture(scope="module")
def report():
    return sweep()


def test_checked_in_blend_boundary_evidence_is_current(report) -> None:

    assert report == json.loads(JSON_REPORT.read_text(encoding="utf-8"))
    assert markdown(report) == MARKDOWN_REPORT.read_text(encoding="utf-8")


def test_sweep_separates_survival_loss_and_reclassification(report) -> None:
    assert report["schema"] == 1
    assert (
        report["baseline_commit"] == BASELINE_COMMIT == ("5569f1405c87be8156e20726152d481623fee6c0")
    )
    assert (
        report["implementation_commit"]
        == IMPLEMENTATION_COMMIT
        == ("50262610a82114276f736baec64278f5fc12b567")
    )
    assert report["radii_role"] == "authored input geometry, not recognition thresholds"
    assert report["performance_budget_seconds"] == PERFORMANCE_BUDGET_SECONDS == 30.0
    assert report["performance_measurement"] == PERFORMANCE_MEASUREMENT
    assert PERFORMANCE_MEASUREMENT["median_seconds"] == 16.098
    assert report["totals"] == {
        "cases": 5,
        "variants": 15,
        "same-family": 6,
        "changed-record": 6,
        "reclassified": 3,
        "absent": 0,
    }


def test_sweep_proves_pad_as_the_second_selected_consumer(report) -> None:
    pad = report["cases"]["rectangular-pad-side-boundary"]
    assert {variant["outcome"] for variant in pad["variants"]} == {"same-family"}
    assert all(
        variant["expected_records"] == pad["plain_records"]["pads"] for variant in pad["variants"]
    )

    boss = report["cases"]["polygonal-boss-side-boundary"]
    assert {variant["outcome"] for variant in boss["variants"]} == {"same-family"}
    assert all(
        variant["expected_records"] == boss["plain_records"]["polygonal_bosses"]
        for variant in boss["variants"]
    )


def test_pocket_reclassification_is_not_reported_as_simple_loss(report) -> None:
    pocket = report["cases"]["blind-pocket-floor-perimeter"]

    assert {variant["outcome"] for variant in pocket["variants"]} == {"reclassified"}
    assert all(
        variant["introduced_families"] == ["prismatic_pockets"] for variant in pocket["variants"]
    )


def test_hole_and_groove_survival_changes_only_authored_span(report) -> None:
    holes = report["cases"]["through-hole-rims"]
    for variant in holes["variants"]:
        radius = variant["radius_model_units"]
        assert variant["outcome"] == "changed-record"
        assert len(variant["expected_records"]) == 2
        for record, x in zip(variant["expected_records"], (-18.0, 18.0), strict=True):
            assert record == {
                "axis": [0.0, 0.0, -1.0],
                "bottom": "through",
                "cbore": None,
                "csink": None,
                "depth": 20.0 - 2.0 * radius,
                "diameter": 8.0,
                "location": [x, 0.0, 10.0 - radius],
                "spotface": None,
            }

    groove = report["cases"]["ring-groove-lead-ins"]
    for variant in groove["variants"]:
        radius = variant["radius_model_units"]
        assert variant["outcome"] == "changed-record"
        assert variant["expected_records"] == [
            {
                "at": [0.0, 0.0, 11.25 + radius / 2.0],
                "axis": "z",
                "diameter": 24.0,
                "width": 2.5 - radius,
            }
        ]
