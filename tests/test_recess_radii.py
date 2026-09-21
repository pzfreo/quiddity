from __future__ import annotations

import pytest
from build123d import (
    Axis,
    Box,
    Pos,
    RectangleRounded,
    Rot,
    export_step,
    extrude,
    fillet,
    import_step,
)

from quiddity._adjacency import FaceGraph
from quiddity._recess_core import _pocket_proposals_one, _slot_proposals_one
from quiddity._recess_faces import _cylinder_faces
from quiddity._recess_radii import _proved_corner_radius
from tests.golden._common import obround_tool


def _rounded_rectangle(radius: float, *, blind: bool, scale: float = 1.0):
    stock = Box(40 * scale, 30 * scale, 10 * scale)
    if blind:
        tool = extrude(RectangleRounded(20 * scale, 10 * scale, radius * scale), 5 * scale)
    else:
        tool = Pos(0, 0, -10 * scale) * extrude(
            RectangleRounded(20 * scale, 10 * scale, radius * scale), 20 * scale
        )
    return stock - tool


def _proof(part, *, blind: bool) -> float | None:
    graph = FaceGraph(part)
    proposals = (
        _pocket_proposals_one(part, graph=graph)
        if blind
        else _slot_proposals_one(part, graph=graph)
    )
    assert len(proposals) == 1
    return _proved_corner_radius(proposals[0].record, _cylinder_faces(part, graph))


@pytest.mark.parametrize("blind", [False, True])
@pytest.mark.parametrize("radius", [1.0, 2.0, 3.0])
@pytest.mark.parametrize("scale", [0.1, 1.0, 10.0])
def test_uniform_four_corner_radius_is_scale_stable(blind, radius, scale) -> None:
    assert _proof(
        _rounded_rectangle(radius, blind=blind, scale=scale), blind=blind
    ) == pytest.approx(radius * scale, abs=0.01)


@pytest.mark.parametrize("blind", [False, True])
def test_uniform_four_corner_radius_survives_step_round_trip(tmp_path, blind) -> None:
    path = tmp_path / ("rounded-pocket.step" if blind else "rounded-slot.step")
    export_step(_rounded_rectangle(2.0, blind=blind), path)
    assert _proof(import_step(path), blind=blind) == 2.0


@pytest.mark.parametrize("rotation", [Rot(90, 0, 0), Rot(0, 90, 0)])
def test_uniform_four_corner_radius_is_principal_axis_invariant(rotation) -> None:
    assert _proof(rotation * _rounded_rectangle(2.0, blind=False), blind=False) == 2.0


def test_square_and_obround_footprints_are_not_four_corner_proofs() -> None:
    square = Box(40, 30, 10) - Box(20, 10, 20)
    obround = Box(40, 30, 10) - obround_tool(20, 10, 20)
    assert _proof(square, blind=False) is None
    assert _proof(obround, blind=False) is None


@pytest.mark.parametrize("rounded_corners", [1, 2, 3])
def test_partial_corner_sets_remain_unknown(rounded_corners) -> None:
    tool = Box(20, 10, 20)
    tool = fillet(list(tool.edges().filter_by(Axis.Z))[:rounded_corners], 2.0)
    assert _proof(Box(40, 30, 10) - tool, blind=False) is None


@pytest.mark.parametrize("first_radius_corners", [1, 2])
def test_mixed_corner_radii_remain_unknown(first_radius_corners) -> None:
    tool = Box(20, 10, 20)
    tool = fillet(list(tool.edges().filter_by(Axis.Z))[:first_radius_corners], 1.0)
    tool = fillet(list(tool.edges().filter_by(Axis.Z)), 2.0)
    assert _proof(Box(40, 30, 10) - tool, blind=False) is None
