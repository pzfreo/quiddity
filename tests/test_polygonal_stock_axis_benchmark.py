"""Reproducibility checks for the E2 principal-axis stock evidence."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

ROOT = Path(__file__).parents[1]
BENCHMARKS = ROOT / "docs" / "benchmarks"
EFFECTIVENESS = BENCHMARKS / "effectiveness-mfcadpp-500-e2-polygonal-stock-16df9c8.json"
E5F = BENCHMARKS / "effectiveness-mfcadpp-500-e5f-6946256.json"
PERFORMANCE = (
    BENCHMARKS / "polygonal-stock-axis-performance-mfcadpp-500-16df9c8.json",
    BENCHMARKS / "polygonal-stock-axis-performance-census-16df9c8.json",
)
IMPLEMENTATION = "16df9c8ce82536515f9e88aad9cb4cf69c9dcea7"


def test_effectiveness_report_pins_neutral_complete_corpus_result() -> None:
    report = json.loads(EFFECTIVENESS.read_text(encoding="utf-8"))
    baseline = json.loads(E5F.read_text(encoding="utf-8"))

    assert report["package"]["commit"] == IMPLEMENTATION
    assert report["selection"]["limit"] == 500
    assert report["summary"]["selected"] == 500
    assert report["summary"]["loaded"] == 500
    assert report["summary"]["evaluated"] == 500
    assert report["summary"]["invalid"] == 0
    assert report["summary"]["empty"] == 0
    assert report["summary"] == baseline["summary"]
    without_seconds = [
        {key: value for key, value in row.items() if key != "seconds"} for row in report["models"]
    ]
    baseline_without_seconds = [
        {key: value for key, value in row.items() if key != "seconds"} for row in baseline["models"]
    ]
    assert without_seconds == baseline_without_seconds


@pytest.mark.parametrize("path", PERFORMANCE)
def test_paired_artifact_pins_exact_parity_and_performance(path: Path) -> None:
    report = json.loads(path.read_text(encoding="utf-8"))

    assert report["implementation_commit"] == IMPLEMENTATION
    assert report["all_pre_existing_outputs_equal"] is True
    assert report["legacy_z_stock"] == 0
    assert report["principal_axis_stock"] == 0
    assert report["enabled_to_disabled_total_ratio"] <= 1.10
    assert len(report["models"]) == (500 if report["workload"] == "mfcadpp" else 13)
    assert all(row["pre_existing_outputs_equal"] for row in report["models"])
