"""Pinned face- and occurrence-level NURBS conversion evidence for issue #276."""

from __future__ import annotations

import json
import math

import pytest

from quiddity._analytic_surfaces import SurfaceKind
from tools.nurbs_conversion_sweep import (
    EXCLUDED_FIXTURES,
    JSON_REPORT,
    MARKDOWN_REPORT,
    PERFORMANCE_BUDGET_SECONDS,
    REVIEWED_DELTA_BOUNDS,
    _parameter_delta,
    _reported_distance,
    markdown,
    sweep,
)


@pytest.fixture(scope="module")
def report():
    return sweep()


def test_checked_in_nurbs_conversion_evidence_is_current(report) -> None:
    assert report == json.loads(JSON_REPORT.read_text(encoding="utf-8"))
    assert markdown(report) == MARKDOWN_REPORT.read_text(encoding="utf-8")


def test_conversion_sweep_proves_face_and_raised_pad_precision(report) -> None:
    assert report["schema"] == 2
    assert report["face_correspondence"].startswith(
        "OCCT BRepBuilderAPI_NurbsConvert.ModifiedShape one-to-one"
    )
    assert report["reviewed_delta_bounds"] == REVIEWED_DELTA_BOUNDS == {
        "face_centre_model_units": 0.1,
        "absolute_face_area_square_units": 25.0,
        "relative_face_area": 0.004,
        "effective_primitive_parameter": 1e-8,
    }
    assert report["excluded_fixtures"] == EXCLUDED_FIXTURES == {
        "circular_blind_step": (
            "whole-solid conversion changes the quarter-cylinder edge signature"
        ),
        "toroidal_blend_compound": (
            "effective-surface recovery deliberately excludes native tori"
        ),
        "toroidal_blend_internal": (
            "effective-surface recovery deliberately excludes native tori"
        ),
        "toroidal_blends_turned": (
            "effective-surface recovery deliberately excludes native tori"
        ),
    }
    assert report["totals"] == {
        "fixtures": 26,
        "faces": 384,
        "native_occurrences": 1,
        "converted_occurrences": 1,
        "same_family": 1,
        "reclassified": 0,
        "changed_record": 0,
        "absent": 0,
        "introduced": 0,
        "recovered_by_primitive": {"cone": 1, "cylinder": 40, "plane": 343},
        "refused_by_reason": {},
    }
    assert all(
        all(fixture["topology"].values()) for fixture in report["fixtures"].values()
    )


def test_parameter_delta_ignores_equivalent_plane_direction_gauge() -> None:
    native = (-0.7071067811865475, 0.7071067811865475, 0.0, 72.12489168102783)
    converted = (0.7071067811865476, -0.7071067811865475, 0.0, -72.12489168102785)

    assert _parameter_delta(SurfaceKind.PLANE, native, converted) < 1e-12


@pytest.mark.parametrize("value, expected", [
    (9.470588235294117e-06, 9.47058824e-06),
    (1.1285714285714287e-05, 1.12857143e-05),
    (0.0010285714285714286, 0.00102857143),
])
def test_report_distance_ignores_quadrature_ulps_but_preserves_meaningful_changes(value, expected):
    assert _reported_distance(value) == expected
    assert _reported_distance(math.nextafter(value, math.inf)) == expected
    assert _reported_distance(math.nextafter(value, -math.inf)) == expected
    assert _reported_distance(value * 1.001) != expected


def test_converted_pad_retains_every_surface_and_material_side_certificate(report) -> None:
    fixture = report["fixtures"]["plates_pads_levels_and_slanted_steps"]
    pad = fixture["raised_pads"]
    assert pad["native_records"] == pad["converted_records"]
    (uses,) = pad["converted_provenance"]
    assert len(uses) == 5
    assert all(use["surface_provenance"] == "recovered" for use in uses)
    assert all(use["recovery"] is not None for use in uses)
    material = [use["material_side"] for use in uses if use["material_side"] is not None]
    assert len(material) == 1
    assert material[0]["outward"] == [0.0, 0.0, 1.0]
    assert material[0]["samples"] >= 2


def test_performance_budget_is_an_explicit_complete_workload_ceiling(report) -> None:
    assert report["performance_budget_seconds"] == PERFORMANCE_BUDGET_SECONDS == 3.0
    measurement = report["performance_measurement"]
    assert measurement["median_seconds"] == 2.133
    assert measurement["repeat"] == 3
    assert "conversion excluded" in measurement["workload"]
