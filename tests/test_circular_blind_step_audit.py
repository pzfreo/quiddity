# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Authored geometry contracts for the circular blind-step evidence audit."""

from __future__ import annotations

import json
import sys
from types import SimpleNamespace

import pytest
from build123d import Box, Cylinder, Pos, Rot

from quiddity._adjacency import FaceGraph
from quiddity._cylinder_substrate import analyse_cylinders
from tools import audit_mfcadpp_circular_blind_steps as audit
from tools.audit_mfcadpp_circular_blind_steps import (
    _QUARTER_TURN_RAD_TOL,
    candidate_pairs,
    describe_component,
    probe_pair,
)


def _circular_blind_step():
    stock = Box(40, 30, 20)
    quarter_cylinder = Pos(7.5, 15, 10) * Rot(0, 90, 0) * Cylinder(4, 25)
    return stock - quarter_cylinder


def _cylinders(part):
    along_z, cross_axis = analyse_cylinders(part)
    return [*along_z, *cross_axis]


def test_quarter_cylinder_open_to_one_envelope_has_one_recognisable_pair() -> None:
    part = _circular_blind_step()
    graph = FaceGraph(part)

    pairs = candidate_pairs(graph, _cylinders(part))

    assert len(pairs) == 1
    cylinder, terminal, probe = pairs[0]
    assert probe.first_failed_gate == "recognisable"
    assert probe.axis == "x"
    assert probe.radius == 4.0
    assert probe.length == 25.0
    assert probe.exact_empty_sweep is True
    assert {graph.face(cylinder).geom_type.name, graph.face(terminal).geom_type.name} == {
        "CYLINDER",
        "PLANE",
    }


def test_component_description_is_node_order_independent() -> None:
    part = _circular_blind_step()
    graph = FaceGraph(part)
    cylinder, terminal, _probe = candidate_pairs(graph, _cylinders(part))[0]

    assert describe_component(graph, (cylinder, terminal), _cylinders(part)) == describe_component(
        graph, (terminal, cylinder), _cylinders(part)
    )


def test_rotation_changes_axis_but_preserves_the_geometry_contract() -> None:
    rotated = Rot(0, 0, 90) * _circular_blind_step()
    graph = FaceGraph(rotated)

    pairs = candidate_pairs(graph, _cylinders(rotated))

    assert len(pairs) == 1
    assert pairs[0][2].axis == "y"


def test_quarter_turn_parameter_tolerance_is_pinned_on_both_sides() -> None:
    part = _circular_blind_step()
    graph = FaceGraph(part)
    cylinders = _cylinders(part)
    cylinder, terminal, _probe = candidate_pairs(graph, cylinders)[0]
    evidence = next(item for item in cylinders if graph.require_node(item["face"]) == cylinder)

    within = dict(evidence)
    within["u_extent"] += _QUARTER_TURN_RAD_TOL / 2
    outside = dict(evidence)
    outside["u_extent"] += _QUARTER_TURN_RAD_TOL * 2

    assert probe_pair(graph, cylinder, terminal, within).first_failed_gate == "recognisable"
    assert (
        probe_pair(graph, cylinder, terminal, outside).first_failed_gate == "not_quarter_cylinder"
    )


def test_full_cylindrical_blind_hole_is_not_a_circular_blind_step() -> None:
    stock = Box(40, 30, 20)
    blind_hole = Pos(0, 0, 5) * Cylinder(4, 15)
    part = stock - blind_hole
    graph = FaceGraph(part)

    assert candidate_pairs(graph, _cylinders(part)) == ()


def test_through_corner_quarter_cylinder_has_no_interior_terminal() -> None:
    stock = Box(40, 30, 20)
    through = Pos(0, 15, 10) * Rot(0, 90, 0) * Cylinder(4, 50)
    part = stock - through
    graph = FaceGraph(part)

    assert candidate_pairs(graph, _cylinders(part)) == ()


def test_command_fails_closed_for_an_empty_dataset_root(tmp_path, monkeypatch) -> None:
    monkeypatch.setattr(
        sys,
        "argv",
        ["audit_mfcadpp_circular_blind_steps.py", str(tmp_path), "--output", str(tmp_path / "x")],
    )

    with pytest.raises(SystemExit, match="2"):
        audit.main()


def test_command_emits_deterministic_report_and_reconciled_arithmetic(
    tmp_path, monkeypatch
) -> None:
    source = tmp_path / "mini.step"
    source.touch()
    part = _circular_blind_step()
    semantic = tuple(21 for _face in part.faces())
    monkeypatch.setattr(audit, "import_step", lambda _path: part)
    monkeypatch.setattr(
        audit, "load_mfcadpp_truth", lambda _path: SimpleNamespace(semantic=semantic)
    )
    monkeypatch.setattr(audit, "_commit", lambda: "authored-test")

    first = tmp_path / "first.json"
    second = tmp_path / "second.json"
    monkeypatch.setattr(
        sys,
        "argv",
        ["audit_mfcadpp_circular_blind_steps.py", str(tmp_path), "--output", str(first)],
    )
    assert audit.main() == 0
    monkeypatch.setattr(
        sys,
        "argv",
        ["audit_mfcadpp_circular_blind_steps.py", str(tmp_path), "--output", str(second)],
    )
    assert audit.main() == 0

    report = json.loads(first.read_text(encoding="utf-8"))
    assert first.read_bytes() == second.read_bytes()
    assert report["implementation_commit"] == "authored-test"
    assert report["selection"]["selected"] == 1
    assert report["labelled_faces"] == len(semantic)
    assert report["derived_components"] == report["recalled_components"] == 1
    assert report["predictions"] == report["true_predictions"] == 1
    assert report["defining_faces"] == report["true_defining_faces"] == 2
    assert sum(report["first_failed_gate_counts"].values()) == report["derived_components"]


def test_command_refuses_invalid_imported_geometry(tmp_path, monkeypatch) -> None:
    source = tmp_path / "invalid.step"
    source.touch()
    monkeypatch.setattr(audit, "import_step", lambda _path: SimpleNamespace(is_valid=False))
    monkeypatch.setattr(audit, "load_mfcadpp_truth", lambda _path: SimpleNamespace(semantic=()))
    monkeypatch.setattr(
        sys,
        "argv",
        ["audit_mfcadpp_circular_blind_steps.py", str(tmp_path), "--output", str(tmp_path / "x")],
    )

    with pytest.raises(RuntimeError, match="invalid"):
        audit.main()


def test_command_refuses_imported_face_label_count_mismatch(tmp_path, monkeypatch) -> None:
    source = tmp_path / "mismatch.step"
    source.touch()
    part = Box(10, 10, 10)
    monkeypatch.setattr(audit, "import_step", lambda _path: part)
    monkeypatch.setattr(
        audit,
        "load_mfcadpp_truth",
        lambda _path: SimpleNamespace(semantic=tuple(21 for _face in part.faces()[:-1])),
    )
    monkeypatch.setattr(
        sys,
        "argv",
        ["audit_mfcadpp_circular_blind_steps.py", str(tmp_path), "--output", str(tmp_path / "x")],
    )

    with pytest.raises(RuntimeError, match="face count"):
        audit.main()
