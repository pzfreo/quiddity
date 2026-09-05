# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Provider-owned corner guards formerly exercised through Draftwright private imports."""

from itertools import permutations
from types import SimpleNamespace

import pytest
from build123d import Align, Box, Pos

from quiddity import build_section_recess_document
from quiddity._recess_core import _recognise_corner_notches
from quiddity._recess_faces import _Face


def bounds(x0, x1, y0, y1, z0, z1):
    return SimpleNamespace(
        min=SimpleNamespace(X=x0, Y=y0, Z=z0), max=SimpleNamespace(X=x1, Y=y1, Z=z1)
    )


def corner_faces(*, x_span=12, y_span=8, wall_depth=(20, 25)):
    return [
        _Face((0, 0, 1), "z", bounds(0, x_span, 0, y_span, 20, 20), True),
        _Face((-1, 0, 0), "x", bounds(x_span, x_span, 0, y_span, *wall_depth), True),
        _Face((0, -1, 0), "y", bounds(0, x_span, y_span, y_span, *wall_depth), True),
    ]


@pytest.mark.parametrize(
    "case", ["degenerate_x", "degenerate_y", "no_walls", "shallow", "split_depth", "short_wall"]
)
def test_degenerate_or_incomplete_corner_faces_are_rejected_in_every_order(case):
    faces = corner_faces()
    if case == "degenerate_x":
        faces = corner_faces(x_span=0)
    elif case == "degenerate_y":
        faces = corner_faces(y_span=0)
    elif case == "no_walls":
        faces = faces[:1]
    elif case == "shallow":
        faces = corner_faces(wall_depth=(10, 15))
    elif case == "split_depth":
        faces[1] = _Face((-1, 0, 0), "x", bounds(12, 12, 0, 8, 20, 23), True)
    else:
        faces[1] = _Face((-1, 0, 0), "x", bounds(12, 12, 0, 3, 20, 25), True)
    for ordered in permutations(faces):
        assert _recognise_corner_notches(list(ordered), bounds(0, 50, 0, 50, 0, 25)) == []


@pytest.mark.parametrize("dimensions", [(12, 8), (8, 12)])
def test_corner_long_axis_and_dimensions_do_not_follow_face_order(dimensions):
    for ordered in permutations(corner_faces(x_span=dimensions[0], y_span=dimensions[1])):
        (record,) = _recognise_corner_notches(list(ordered), bounds(0, 50, 0, 50, 0, 25))
        assert (record.width, record.length, record.depth) == (8, 12, 5)
        assert record.long_axis == ("x" if dimensions[0] > dimensions[1] else "y")
        assert record.width_axis == ("y" if dimensions[0] > dimensions[1] else "x")
        assert record.edge_anchored


def test_public_corner_retains_longer_x_span_in_its_open_profile():
    align = (Align.MIN, Align.MIN, Align.MIN)
    part = Box(50, 50, 25, align=align) - Pos(0, 0, 20) * Box(12, 8, 5, align=align)
    document = build_section_recess_document(part)
    assert document.refusals == ()
    (record,) = document.occurrences
    assert record.classification.feature_kind == "edge_open_recess"
    assert record.geometry.profile.closure == "open"
    assert record.geometry.run_interval[1] - record.geometry.run_interval[0] == 5
    frame = record.geometry.frame
    points = [
        tuple(
            frame.origin[i] + frame.u[i] * vertex.point[0] + frame.v[i] * vertex.point[1]
            for i in range(3)
        )
        for vertex in record.geometry.profile.boundary
    ]
    assert max(p[0] for p in points) - min(p[0] for p in points) == 12
    assert max(p[1] for p in points) - min(p[1] for p in points) == 8
