# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

from __future__ import annotations

import json

from build123d import Box, Edge, Face, Pos, Rot, Shell, Solid, Wire

from quiddity._adjacency import FaceGraph
from quiddity._candidates import (
    FamilyId,
    PredicateId,
    SplitTriangularTerminalFact,
)
from quiddity._claims import ClaimLedger
from quiddity._diagnostics import (
    DiagnosticCode,
    DiagnosticStatus,
    diagnose_residuals,
)
from quiddity._dispositions import ReconciliationResult
from quiddity.chamfers import Chamfer
from quiddity.result import _take_inventory


def _diagnostic_run(
    *,
    contested: bool = False,
    structural_owner: bool = False,
    duplicate: bool = False,
    reverse: bool = False,
):
    ledger = ClaimLedger(FaceGraph(Box(10, 10, 10)))
    subject, terminal = ledger.graph.nodes[:2]
    chamfer = Chamfer("x", 2.0, 2.0, 45.0, (1.0, 2.0, 3.0))
    ledger.propose(FamilyId.CHAMFERS, chamfer, [subject])
    physical = [ledger.candidate_set(FamilyId.CHAMFERS)]
    if contested:
        pocket = object()
        ledger.propose(FamilyId.POCKETS, pocket, [subject, ledger.graph.nodes[2]])
        physical.append(ledger.candidate_set(FamilyId.POCKETS))
    if structural_owner:
        ledger.propose(FamilyId.RISERS, object(), [subject])
        physical.append(ledger.candidate_set(FamilyId.RISERS))
    raw_counts = ([4, 5] if reverse else [5, 4]) if duplicate else [5]
    for raw_count in raw_counts:
        ledger.sink.observe(
            FamilyId.ANGLED_STEPS,
            PredicateId.ANGLED_STEP_TERMINAL,
            subject=subject,
            consulted=[terminal],
            fact=SplitTriangularTerminalFact(raw_count),
        )
    evidence = ledger.freeze_index()
    reconciliation = ReconciliationResult.complete(tuple(physical), (), evidence)
    return diagnose_residuals(reconciliation, evidence)


def test_split_terminal_observation_projects_one_private_json_diagnostic() -> None:
    diagnostics = _diagnostic_run()

    assert len(diagnostics) == 1
    diagnostic = diagnostics[0]
    assert diagnostic.code is DiagnosticCode.UNSUPPORTED_SUBDIVIDED_ANGLED_STEP_TERMINAL
    assert diagnostic.status is DiagnosticStatus.UNSUPPORTED
    assert diagnostic.family == "angled_steps"
    assert diagnostic.axis == "x"
    assert diagnostic.at == (1.0, 2.0, 3.0)
    assert diagnostic.raw_outer_edges == 5
    assert json.loads(json.dumps(diagnostic.to_dict())) == diagnostic.to_dict()


def test_known_non_chamfer_owner_suppresses_the_residual_diagnostic() -> None:
    assert _diagnostic_run(contested=True) == ()


def test_neutral_structural_owner_does_not_suppress_feature_gap_diagnostic() -> None:
    assert len(_diagnostic_run(structural_owner=True)) == 1


def test_multiple_terminal_observations_emit_one_diagnostic_per_slant() -> None:
    forward = _diagnostic_run(duplicate=True)
    reverse = _diagnostic_run(duplicate=True, reverse=True)
    assert len(forward) == 1
    assert forward == reverse
    assert forward[0].raw_outer_edges == 4


def _side_subdivided_blind_step() -> Solid:
    part = Box(60, 40, 12) - Pos(-20, 20, 6) * Rot(45, 0, 0) * Box(30, 5.657, 5.657)
    faces = list(part.faces())
    slant = next(
        face for face in faces if abs(face.normal_at().Y) > 0.5 and abs(face.normal_at().Z) > 0.5
    )
    terminal = next(
        face for face in faces if len(face.edges()) == 3 and abs(face.normal_at().X) > 0.9
    )
    common = next(
        first for first in terminal.edges() for second in slant.edges() if first.is_same(second)
    )
    start, end = (vertex.center() for vertex in common.vertices())
    middle = (start + end) * 0.5
    split_edges = [Edge.make_line(start, middle), Edge.make_line(middle, end)]
    new_terminal = Face(
        Wire([edge for edge in terminal.edges() if not edge.is_same(common)] + split_edges)
    )
    new_slant = Face(
        Wire([edge for edge in slant.edges() if not edge.is_same(common)] + split_edges)
    )
    if new_terminal.normal_at().dot(terminal.normal_at()) < 0:
        new_terminal = Face(new_terminal.outer_wire().reversed())
    if new_slant.normal_at().dot(slant.normal_at()) < 0:
        new_slant = Face(new_slant.outer_wire().reversed())
    result = Solid(
        Shell(
            [face for face in faces if not face.is_same(slant) and not face.is_same(terminal)]
            + [new_terminal, new_slant]
        )
    )
    assert result.is_valid
    return result


def test_split_terminal_now_flows_through_the_real_aggregate() -> None:
    part = _side_subdivided_blind_step()
    product = _take_inventory(part)

    assert len(product.result.angled_steps) == 1
    assert len(product.physical.candidate_set(FamilyId.ANGLED_STEPS).candidates) == 1
    assert (
        product.evidence.observations(FamilyId.ANGLED_STEPS, PredicateId.ANGLED_STEP_TERMINAL) == ()
    )
    assert product.result.chamfers == ()
    assert product.diagnostics == ()
    (candidate,) = product.physical.candidate_set(FamilyId.ANGLED_STEPS).candidates
    assert len(product.evidence.defining_of(candidate)) == 1
    assert len(product.evidence.constituent_of(candidate)) == 2


def test_stock_supported_step_and_plain_chamfer_emit_no_residuals() -> None:
    blind = Box(60, 40, 12) - Pos(-20, 20, 6) * Rot(45, 0, 0) * Box(30, 5.657, 5.657)
    through = Box(60, 40, 12) - Pos(0, 20, 6) * Rot(45, 0, 0) * Box(70, 5.657, 5.657)

    assert _take_inventory(Box(10, 10, 10)).diagnostics == ()
    assert _take_inventory(blind).diagnostics == ()
    assert _take_inventory(through).diagnostics == ()
