"""Authored controls for the one-ended polygonal Pocket audit."""

import ast
import inspect

from build123d import (
    Box,
    BuildPart,
    BuildSketch,
    Compound,
    Cylinder,
    Plane,
    Pos,
    RegularPolygon,
    Rot,
    extrude,
)

from quiddity._adjacency import FaceGraph
from quiddity._sections import PlanarSection, SectionVertex
from tools.audit_mfcadpp_one_ended_pockets import (
    _audit_model,
    _one_ended_regions,
    _without_collinear_subdivisions,
)


def test_full_geometric_probe_roster_precedes_normal_path_label_read() -> None:
    tree = ast.parse(inspect.getsource(_audit_model))
    assignments = {
        target.id: node.lineno
        for node in ast.walk(tree)
        if isinstance(node, (ast.Assign, ast.AnnAssign))
        for target in (node.targets if isinstance(node, ast.Assign) else (node.target,))
        if isinstance(target, ast.Name)
    }
    label_reads = [
        node.lineno
        for node in ast.walk(tree)
        if isinstance(node, ast.Call)
        and isinstance(node.func, ast.Name)
        and node.func.id == "load_mfcadpp_truth"
    ]

    assert len(label_reads) == 2
    assert assignments["probes"] < max(label_reads)


def _hexagonal_tool(*, depth: float, both: bool = False, z: float = 0.0):
    with BuildPart() as tool:
        with BuildSketch(Plane.XY.offset(z)):
            RegularPolygon(7, 6)
        extrude(amount=depth, both=both)
    return tool.part


def _candidates(part, *, expected_sides: int = 6):
    return tuple(
        candidate
        for probe, candidate in _one_ended_regions(FaceGraph(part), expected_sides)
        if candidate
    )


def test_blind_hexagonal_pocket_has_one_mouth_and_one_floor() -> None:
    part = Box(60, 40, 20) - _hexagonal_tool(depth=10)

    (candidate,) = _candidates(part)

    assert len(candidate.section.boundary) == 6
    assert len(candidate.floor) == 1
    assert abs(candidate.floor_at - candidate.mouth_at) == 10


def test_through_and_enclosed_hexagonal_voids_are_not_one_ended() -> None:
    through = Box(60, 40, 20) - _hexagonal_tool(depth=30, both=True)
    enclosed = Box(60, 40, 20) - _hexagonal_tool(depth=10, z=-5)

    assert _candidates(through) == ()
    assert _candidates(enclosed) == ()


def test_floor_breach_is_not_a_bounded_one_ended_pocket() -> None:
    pocket = Box(60, 40, 20) - _hexagonal_tool(depth=10)
    breached = pocket - Pos(0, 0, -5) * Cylinder(2, 20)

    assert _candidates(breached) == ()


def test_polygonal_mouth_audit_uses_the_selected_dataset_family_side_count() -> None:
    triangular = Box(60, 40, 20) - _polygon_tool(3)
    rectangular = Box(60, 40, 20) - Pos(0, 0, 5) * Box(12, 8, 10)

    assert _candidates(triangular) == ()
    assert _candidates(rectangular) == ()
    (triangle,) = _candidates(triangular, expected_sides=3)
    (rectangle,) = _candidates(rectangular, expected_sides=4)
    assert len(triangle.section.boundary) == 3
    assert len(rectangle.section.boundary) == 4


def _polygon_tool(sides: int):
    with BuildPart() as tool:
        with BuildSketch(Plane.XY):
            RegularPolygon(7, sides)
        extrude(amount=10)
    return tool.part


def test_candidate_is_covariant_and_body_local() -> None:
    source = Box(60, 40, 20) - _hexagonal_tool(depth=10)
    moved = Pos(17, -11, 9) * Rot(90, 0, 0) * source
    compound = Compound([source, Pos(100, 0, 0) * source])

    assert len(_candidates(moved)) == 1
    graph = FaceGraph(compound)
    candidates = tuple(candidate for _probe, candidate in _one_ended_regions(graph) if candidate)
    assert len(candidates) == 2
    assert graph.common_valid_solid(candidates[0].region | candidates[1].region) is None


def test_only_codirected_collinear_boundary_subdivisions_are_collapsed() -> None:
    split_hexagon = PlanarSection(
        tuple(
            SectionVertex(point)
            for point in ((0, 0), (1, 0), (2, 0), (3, 1), (2, 2), (0, 2), (-1, 1))
        )
    )
    kinked = PlanarSection(
        tuple(
            SectionVertex(point)
            for point in ((0, 0), (1, 0.01), (2, 0), (3, 1), (2, 2), (0, 2), (-1, 1))
        )
    )

    collapsed = _without_collinear_subdivisions(split_hexagon)
    retained_kink = _without_collinear_subdivisions(kinked)

    assert collapsed is not None and len(collapsed.boundary) == 6
    assert retained_kink is not None and len(retained_kink.boundary) == 7
