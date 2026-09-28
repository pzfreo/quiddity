# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Deterministic authored-geometry tests for the through-step miss audit."""

from __future__ import annotations

from dataclasses import asdict

from build123d import Box, Pos, Rot, Shell

from quiddity._adjacency import FaceGraph
from quiddity._candidates import FamilyId
from quiddity._dispositions import Outcome
from quiddity.result import _take_inventory
from tools.audit_mfcadpp_through_steps import (
    _is_two_wall_boundary_interruption,
    _rank_clusters,
    describe_component,
)


def _step():
    return Box(40, 30, 20) - Pos(15, 10, 0) * Box(20, 20, 30)


def test_accepted_authored_step_has_recognisable_audit_anatomy() -> None:
    product = _take_inventory(_step())
    accepted = [
        item
        for item in product.reconciliation.for_family(FamilyId.THROUGH_STEPS)
        if item.outcome is Outcome.ACCEPTED
    ]
    assert len(accepted) == 1
    nodes = tuple(product.evidence.defining_of(accepted[0].candidate))

    anatomy = describe_component(product.context.graph, nodes)

    assert anatomy.face_count == 2
    assert anatomy.first_failed_gate == "recognisable"
    assert anatomy.full_run_faces == 2
    assert anatomy.terminal_count == 2
    assert anatomy.exact_empty_prism is True


def test_component_descriptor_is_independent_of_supplied_node_order() -> None:
    product = _take_inventory(_step())
    disposition = next(
        item
        for item in product.reconciliation.for_family(FamilyId.THROUGH_STEPS)
        if item.outcome is Outcome.ACCEPTED
    )
    nodes = tuple(product.evidence.defining_of(disposition.candidate))

    assert describe_component(product.context.graph, nodes) == describe_component(
        product.context.graph, tuple(reversed(nodes))
    )


def test_motif_key_is_rotation_neutral() -> None:
    first = _take_inventory(_step())
    second = _take_inventory(Rot(90, 0, 0) * _step())

    def anatomy(product):
        disposition = next(
            item
            for item in product.reconciliation.for_family(FamilyId.THROUGH_STEPS)
            if item.outcome is Outcome.ACCEPTED
        )
        return describe_component(
            product.context.graph,
            tuple(product.evidence.defining_of(disposition.candidate)),
        )

    left = anatomy(first)
    right = anatomy(second)
    assert left.inferred_run_axis != right.inferred_run_axis
    assert left.key() == right.key()


def test_single_authored_face_has_no_inferred_rectangular_pair() -> None:
    part = Box(10, 20, 30)
    graph = FaceGraph(part)
    node = min(graph.nodes, key=lambda item: item.index)

    anatomy = describe_component(graph, (node,))

    assert anatomy.face_count == 1
    assert anatomy.first_failed_gate == "no_orthogonal_rectangular_pair"
    assert anatomy.inferred_run_axis is None
    assert anatomy.exact_empty_prism is None


def test_open_shell_is_recorded_as_unproven_anatomy() -> None:
    graph = FaceGraph(Shell(list(_step().faces())))

    anatomy = describe_component(graph, tuple(graph.nodes[:2]))

    assert anatomy.face_count == 2
    assert anatomy.first_failed_gate == "unproven_solid"
    assert anatomy.exact_empty_prism is None


def test_cluster_ranking_and_samples_ignore_input_traversal_order() -> None:
    product = _take_inventory(_step())
    disposition = next(
        item
        for item in product.reconciliation.for_family(FamilyId.THROUGH_STEPS)
        if item.outcome is Outcome.ACCEPTED
    )
    anatomy = describe_component(
        product.context.graph,
        tuple(product.evidence.defining_of(disposition.candidate)),
    )
    rows = [
        {
            "model_id": model_id,
            "face_indices": indices,
            "face_count": 2,
            "anatomy_key": anatomy.key(),
            "anatomy": asdict(anatomy),
        }
        for model_id, indices in (("b", [8, 9]), ("a", [4, 7]), ("c", [1, 2]))
    ]

    assert _rank_clusters(rows) == _rank_clusters(list(reversed(rows)))
    assert _rank_clusters(rows)[0]["sample_components"][0] == {
        "model_id": "a",
        "face_indices": [4, 7],
    }


def test_broad_interruption_motif_requires_every_remaining_safety_proof() -> None:
    product = _take_inventory(_step())
    disposition = next(
        item
        for item in product.reconciliation.for_family(FamilyId.THROUGH_STEPS)
        if item.outcome is Outcome.ACCEPTED
    )
    anatomy = asdict(
        describe_component(
            product.context.graph,
            tuple(product.evidence.defining_of(disposition.candidate)),
        )
    )
    anatomy["first_failed_gate"] = "nonrectangular_regions"
    anatomy["rectangular_outer_faces"] = 1

    assert _is_two_wall_boundary_interruption(anatomy)
    anatomy["exact_empty_prism"] = False
    assert not _is_two_wall_boundary_interruption(anatomy)
