"""Pinned occurrence-level rigid-motion evidence (#272, epic 0004 F0)."""

from __future__ import annotations

import json

import pytest

from tools.rigid_motion_sweep import (
    JSON_REPORT,
    MARKDOWN_REPORT,
    Occurrence,
    _match,
    markdown,
    sweep,
)


@pytest.fixture(scope="module")
def report():
    return sweep()


def test_occurrence_matcher_maximises_cardinality_before_overlap() -> None:
    baseline = (
        Occurrence("a", frozenset({1})),
        Occurrence("b", frozenset({1, 2})),
    )
    rotated = (
        Occurrence("b", frozenset({1, 2})),
        Occurrence("c", frozenset({2})),
    )

    pairs, absent, introduced = _match(baseline, rotated)

    assert pairs == ((0, 0), (1, 1))
    assert absent == introduced == ()


def test_checked_in_rigid_motion_evidence_is_current(report) -> None:
    assert report == json.loads(JSON_REPORT.read_text(encoding="utf-8"))
    assert markdown(report) == MARKDOWN_REPORT.read_text(encoding="utf-8")


def test_rigid_motion_baseline_separates_absence_from_reclassification(report) -> None:
    assert report["totals"] == {
        "Z30": {
            "baseline_records": 100,
            "retained_same_family": 55,
            "reclassified": 18,
            "absent": 27,
            "introduced": 0,
        },
        "X30": {
            "baseline_records": 100,
            "retained_same_family": 49,
            "reclassified": 11,
            "absent": 40,
            "introduced": 0,
        },
        "X90": {
            "baseline_records": 100,
            "retained_same_family": 100,
            "reclassified": 0,
            "absent": 0,
            "introduced": 0,
        },
    }
    fixtures = report["fixtures"]
    # Native torus-axis precision preserves these three Blends under X30 (#491).
    for name, count in (("toroidal_blend_compound", 2), ("toroidal_blend_internal", 1)):
        rotated = fixtures[name]["rotations"]["X30"]
        assert rotated["same_family"]["blend"] == count
        assert rotated["absent"] == rotated["introduced"] == rotated["reclassified"] == 0
    assert fixtures["blind_pockets_and_pocket_patterns"]["rotations"]["Z30"]["transitions"] == {
        "pocket->prismatic_pocket": 6
    }
    assert fixtures["straight_and_obround_slots"]["rotations"]["Z30"]["transitions"] == {
        "slot->oriented_slot": 4
    }
