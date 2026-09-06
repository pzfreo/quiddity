# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Authored-geometry guards for the paired-ramp miss audit."""

from __future__ import annotations

from contextlib import suppress
from dataclasses import asdict

import pytest
from build123d import Box, Compound, Cylinder, Plane, Polygon, Pos, Rot, extrude

from quiddity._adjacency import FaceGraph
from quiddity._bevel import BevelReject, classify_bevel
from quiddity._candidates import FamilyId
from quiddity._dispositions import Outcome
from quiddity.result import _take_inventory
from tools import audit_mfcadpp_paired_ramp_steps as audit_module
from tools.audit_mfcadpp_paired_ramp_steps import (
    _describe_component,
    _probe_pair,
    _ramp_boundary_bypass_pairs,
    _rank,
    _reconciliation,
    _source_selection_hash,
)


def _side_cut():
    stock = Box(40, 40, 30)
    cutter = Pos(20, 20, 0) * extrude(Plane.XZ * Polygon((0, -8), (0, 8), (-10, 0)), 25)
    return stock - cutter


def _accepted_anatomy(part):
    product = _take_inventory(part)
    disposition = next(
        item
        for item in product.reconciliation.for_family(FamilyId.PAIRED_RAMP_STEPS)
        if item.outcome is Outcome.ACCEPTED
    )
    nodes = tuple(product.evidence.defining_of(disposition.candidate))
    return _describe_component(product.context.graph, nodes, {})


def test_accepted_authored_pair_reaches_the_final_audit_gate() -> None:
    anatomy = _accepted_anatomy(_side_cut())

    assert anatomy.face_count == 3
    assert anatomy.bevel_faces == 2
    assert anatomy.best_pair.first_failed_gate == "recognisable"
    assert anatomy.best_pair.ramp_edge_counts == (4, 4)
    assert anatomy.best_pair.common_axis_terminal_count == 2
    assert anatomy.best_pair.internal_terminal_edges in (3, 5)
    assert anatomy.best_pair.full_shared_run is True


def test_component_descriptor_is_order_and_axis_permutation_neutral() -> None:
    first = _accepted_anatomy(_side_cut())
    second = _accepted_anatomy(Rot(0, 0, 90) * _side_cut())

    assert first.best_pair.run_axis != second.best_pair.run_axis
    assert first.key() == second.key()


def test_cluster_ranking_and_samples_are_deterministic() -> None:
    anatomy = _accepted_anatomy(_side_cut())
    rows = [
        {
            "model_id": model_id,
            "face_indices": indices,
            "face_count": anatomy.face_count,
            "anatomy_key": anatomy.key(),
            "anatomy": asdict(anatomy),
        }
        for model_id, indices in (("b", [7, 8, 9]), ("a", [1, 2, 3]), ("c", [4, 5, 6]))
    ]

    assert _rank(rows) == _rank(list(reversed(rows)))
    assert _rank(rows)[0]["samples"][0] == {
        "model_id": "a",
        "face_indices": [1, 2, 3],
    }


def test_source_selection_hash_pins_content_not_only_model_ids() -> None:
    first = _source_selection_hash([("100", "aaa"), ("200", "bbb")])

    assert first == _source_selection_hash([("100", "aaa"), ("200", "bbb")])
    assert first != _source_selection_hash([("100", "aaa"), ("200", "changed")])


def test_drilled_terminal_reaches_the_final_gate_with_full_run() -> None:
    part = _side_cut() - Pos(15, -5, 0) * Rot(90, 0, 0) * Cylinder(1, 6)
    graph = FaceGraph(part)
    bevels = {}
    for node in graph.nodes:
        with suppress(BevelReject):
            bevels[node] = classify_bevel(graph.face(node))

    probes = [
        _probe_pair(graph, left, right, bevels[left], bevels[right])
        for left in graph.nodes
        for right in graph.neighbours(left)
        if right.index > left.index and left in bevels and right in bevels
    ]
    terminal = [probe for probe in probes if probe.first_failed_gate == "recognisable"]

    assert len(terminal) == 1
    assert terminal[0].full_shared_run is True
    assert terminal[0].internal_terminal_edges not in (3, 5)


def test_subdivided_ramp_uses_the_ordinary_production_path(monkeypatch) -> None:
    graph = FaceGraph(_side_cut())
    bevels = {}
    for node in graph.nodes:
        with suppress(BevelReject):
            bevels[node] = classify_bevel(graph.face(node))
    left, right = next(
        (left, right)
        for left in graph.nodes
        for right in graph.neighbours(left)
        if right.index > left.index
        and left in bevels
        and right in bevels
        and _probe_pair(graph, left, right, bevels[left], bevels[right]).first_failed_gate
        == "recognisable"
    )
    original_edges = graph.edges
    right_edges = set(original_edges(right))
    extra = next(edge for edge in original_edges(left) if edge not in right_edges)

    def subdivided_edges(node):
        edges = original_edges(node)
        return edges + (extra,) if node == left else edges

    monkeypatch.setattr(graph, "edges", subdivided_edges)

    ordinary = _probe_pair(graph, left, right, bevels[left], bevels[right])
    bypass = _ramp_boundary_bypass_pairs(graph, tuple(set((left, right, *graph.neighbours(left)))))

    assert ordinary.first_failed_gate == "recognisable"
    assert bypass == ()


@pytest.mark.parametrize(
    ("failure", "expected"),
    [
        ("ridge_arc", "ridge_not_concave"),
        ("terminal_planarity", "not_two_common_axis_terminals"),
        ("solid_ownership", "cross_solid_or_invalid_solid"),
        ("complete_run", "incomplete_shared_run"),
    ],
)
def test_ramp_boundary_bypass_retains_downstream_refusals(
    monkeypatch, failure: str, expected: str
) -> None:
    graph = FaceGraph(_side_cut())
    bevels = {}
    for node in graph.nodes:
        with suppress(BevelReject):
            bevels[node] = classify_bevel(graph.face(node))
    left, right = next(
        (left, right)
        for left in graph.nodes
        for right in graph.neighbours(left)
        if right.index > left.index
        and left in bevels
        and right in bevels
        and _probe_pair(graph, left, right, bevels[left], bevels[right]).first_failed_gate
        == "recognisable"
    )
    original_edges = graph.edges
    right_edges = set(original_edges(right))
    extra = next(edge for edge in original_edges(left) if edge not in right_edges)
    monkeypatch.setattr(
        graph,
        "edges",
        lambda node: original_edges(node) + (extra,) if node == left else original_edges(node),
    )
    left_read = bevels[left]
    if failure == "ridge_arc":
        monkeypatch.setattr(audit_module, "_is_concave", lambda *_args: False)
    elif failure == "terminal_planarity":
        monkeypatch.setattr(graph, "is_planar", lambda _node: False)
    elif failure == "solid_ownership":
        monkeypatch.setattr(graph, "common_valid_solid", lambda _nodes: None)
    else:
        axis, normal, spans, high, low = left_read
        incomplete = dict(spans)
        incomplete[axis] = (spans[axis][0], spans[axis][1] + 1.0)
        left_read = (axis, normal, incomplete, high, low)

    result = _probe_pair(
        graph,
        left,
        right,
        left_read,
        bevels[right],
        bypass_ramp_boundary=True,
    )

    assert result.first_failed_gate == expected


def test_subdivided_ramp_multiplicity_needs_no_legacy_bypass(monkeypatch) -> None:
    graph = FaceGraph(Compound([_side_cut(), Pos(100, 0, 0) * _side_cut()]))
    bevels = {}
    for node in graph.nodes:
        with suppress(BevelReject):
            bevels[node] = classify_bevel(graph.face(node))
    pairs = [
        (left, right)
        for left in graph.nodes
        for right in graph.neighbours(left)
        if right.index > left.index
        and left in bevels
        and right in bevels
        and _probe_pair(graph, left, right, bevels[left], bevels[right]).first_failed_gate
        == "recognisable"
    ]
    assert len(pairs) == 2
    original_edges = graph.edges
    extras = {
        left: next(edge for edge in original_edges(left) if edge not in set(original_edges(right)))
        for left, right in pairs
    }
    monkeypatch.setattr(
        graph,
        "edges",
        lambda node: (
            original_edges(node) + (extras[node],) if node in extras else original_edges(node)
        ),
    )

    forward = _ramp_boundary_bypass_pairs(graph, tuple(graph.nodes))
    reverse = _ramp_boundary_bypass_pairs(graph, tuple(reversed(graph.nodes)))

    assert forward == reverse
    assert forward == ()


def test_reconciliation_retains_residual_faces_in_touched_components() -> None:
    rows = [
        {
            "matched_face_indices": [1, 2, 3],
            "unmatched_face_indices": [4, 5],
        },
        {"matched_face_indices": [], "unmatched_face_indices": [6, 7, 8]},
    ]

    assert _reconciliation(rows, 8) == {
        "labelled_faces": 8,
        "matched_defining_faces": 3,
        "unmatched_labelled_faces": 5,
        "derived_components": 2,
        "recalled_components": 1,
        "unrecalled_components": 1,
        "partially_recalled_components": 1,
    }
    with pytest.raises(RuntimeError, match="face reconciliation failed"):
        _reconciliation(rows, 9)
