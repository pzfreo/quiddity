# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Coplanar stock patches are one physical mouth, not extra pocket evidence."""

from __future__ import annotations

import pytest
from build123d import (
    Box,
    BuildSketch,
    Keep,
    Plane,
    Pos,
    RegularPolygon,
    Rot,
    Shell,
    Solid,
    extrude,
)

from quiddity import build_section_recess_document
from quiddity._adjacency import FaceGraph
from quiddity._section_recess_geometry import _one_polygonal_candidate


def _pocket(sides: int, *, split: bool):
    with BuildSketch() as sketch:
        RegularPolygon(7, sides)
    part = Box(40, 40, 20) - extrude(sketch.sketch, amount=15)
    if not split:
        return part
    faces = []
    for face in part.faces():
        if abs(face.center().Z - 10) < 1e-6:
            faces.extend(face.split(Plane.YZ, Keep.BOTH))
        else:
            faces.append(face)
    rebuilt = Solid(Shell(faces))
    assert rebuilt.is_valid
    return rebuilt


def _native(part):
    graph = FaceGraph(part)
    candidates = [
        candidate
        for node in graph.nodes
        if graph.is_planar(node)
        and (candidate := _one_polygonal_candidate(graph, node)) is not None
    ]
    return graph, candidates


@pytest.mark.parametrize("sides", (3, 4, 6))
@pytest.mark.parametrize("placement", (Rot(), Pos(17, -11, 9) * Rot(31, 17, 23)))
def test_split_coplanar_mouth_preserves_native_pocket(sides, placement):
    _, baseline = _native(placement * _pocket(sides, split=False))
    graph, split = _native(placement * _pocket(sides, split=True))
    assert len(baseline) == len(split) == 1
    assert split[0].geometry == baseline[0].geometry
    assert split[0].section_shape == baseline[0].section_shape
    assert len(split[0].defining_faces) == sides
    assert len(split[0].constituent_faces) == sides + 1
    assert (
        graph.common_valid_solid(
            tuple(node for node in graph.nodes if node.index in split[0].constituent_faces)
        )
        is not None
    )


@pytest.mark.parametrize("scale", (0.1, 10.0))
def test_split_mouth_proof_scales_without_changing_profile(scale):
    _, baseline = _native(_pocket(6, split=False).scale(scale))
    _, split = _native(_pocket(6, split=True).scale(scale))
    assert len(split) == len(baseline) == 1
    assert split[0].geometry == baseline[0].geometry


def test_every_wall_requires_mouth_context(monkeypatch):
    graph, (candidate,) = _native(_pocket(6, split=True))
    unsupported = candidate.defining_faces[0]
    arc = FaceGraph.arc

    def missing_context(self, left, right):
        result = arc(self, left, right)
        if unsupported in (left.index, right.index) and result in ("convex", "smooth"):
            return None
        return result

    monkeypatch.setattr(FaceGraph, "arc", missing_context)
    assert not any(
        _one_polygonal_candidate(graph, node) is not None
        for node in graph.nodes
        if graph.is_planar(node)
    )


def test_stepped_stock_does_not_become_one_flat_mouth():
    part = _pocket(6, split=False) - Pos(10, 0, 8) * Box(20, 40, 4)
    assert part.is_valid
    assert _native(part)[1] == []


def test_public_document_excludes_consulted_stock_patches():
    part = _pocket(6, split=True)
    graph, (native,) = _native(part)
    document = build_section_recess_document(part)
    matches = [
        record
        for record in document.occurrences
        if record.classification.section_shape == "hexagonal"
    ]
    assert len(matches) == 1
    record = matches[0]
    assert set(record.evidence.defining_faces) == set(native.defining_faces)
    assert set(record.evidence.constituent_faces) == set(native.constituent_faces)
    stock = {node.index for node in graph.nodes if abs(graph.face(node).center().Z - 10) < 1e-6}
    assert stock.isdisjoint(record.evidence.constituent_faces)
